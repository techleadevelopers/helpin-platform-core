use anyhow::Context;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use uuid::Uuid;

const ADMIN_PASSWORD: &str = "Admin@";
const ONG_PASSWORD: &str = "Ong@123456";
const USER_PASSWORD: &str = "Usuario@123456";

struct SeedUser {
    id: &'static str,
    name: &'static str,
    email: &'static str,
    avatar_url: &'static str,
    account_type: &'static str,
    verified: bool,
    trust_score: i16,
    gender: Option<&'static str>,
    cep: &'static str,
    street: &'static str,
    number: &'static str,
    neighborhood: &'static str,
    city: &'static str,
    state: &'static str,
    password: &'static str,
}

struct SeedOng {
    id: &'static str,
    user_id: &'static str,
    legal_name: &'static str,
    cnpj: &'static str,
    mission: &'static str,
    city: &'static str,
    state: &'static str,
    area_type: &'static str,
    contact_phone: &'static str,
    cep: &'static str,
    street: &'static str,
    number: &'static str,
    neighborhood: &'static str,
    foundation_year: i32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .context("failed to connect to database")?;

    seed_users(&pool).await?;
    seed_ongs(&pool).await?;
    print_summary(&pool).await?;

    Ok(())
}

async fn seed_users(pool: &PgPool) -> anyhow::Result<()> {
    for user in users() {
        let password_hash = hash_password(user.password)?;
        sqlx::query(
            r#"
            INSERT INTO users (
              id, name, email, avatar_url, password_hash, account_type, verified,
              trust_score, gender, cep, street, number, complement, neighborhood, city, state
            )
            VALUES (
              $1, $2, $3, $4, $5, $6::account_type, $7,
              $8, $9, $10, $11, $12, NULL, $13, $14, $15
            )
            ON CONFLICT (email) DO UPDATE SET
              name = EXCLUDED.name,
              avatar_url = EXCLUDED.avatar_url,
              password_hash = EXCLUDED.password_hash,
              account_type = EXCLUDED.account_type,
              verified = EXCLUDED.verified,
              trust_score = EXCLUDED.trust_score,
              gender = EXCLUDED.gender,
              cep = EXCLUDED.cep,
              street = EXCLUDED.street,
              number = EXCLUDED.number,
              neighborhood = EXCLUDED.neighborhood,
              city = EXCLUDED.city,
              state = EXCLUDED.state,
              deleted_at = NULL,
              anonymized_at = NULL,
              retention_delete_after = NULL
            "#,
        )
        .bind(Uuid::parse_str(user.id)?)
        .bind(user.name)
        .bind(user.email)
        .bind(user.avatar_url)
        .bind(password_hash)
        .bind(user.account_type)
        .bind(user.verified)
        .bind(user.trust_score)
        .bind(user.gender)
        .bind(user.cep)
        .bind(user.street)
        .bind(user.number)
        .bind(user.neighborhood)
        .bind(user.city)
        .bind(user.state)
        .execute(pool)
        .await
        .with_context(|| format!("failed to upsert user {}", user.email))?;
    }

    Ok(())
}

async fn seed_ongs(pool: &PgPool) -> anyhow::Result<()> {
    for ong in ongs() {
        sqlx::query(
            r#"
            INSERT INTO ong_profiles (
              id, user_id, legal_name, cnpj, mission, city, state, latitude, longitude,
              verified_at, area_type, contact_phone, cep, street, number, complement,
              neighborhood, foundation_year, verification_status, verification_reviewed_at
            )
            VALUES (
              $1, $2, $3, $4, $5, $6, $7, NULL, NULL,
              now(), $8, $9, $10, $11, $12, NULL,
              $13, $14, 'APPROVED', now()
            )
            ON CONFLICT (cnpj) DO UPDATE SET
              user_id = EXCLUDED.user_id,
              legal_name = EXCLUDED.legal_name,
              mission = EXCLUDED.mission,
              city = EXCLUDED.city,
              state = EXCLUDED.state,
              verified_at = COALESCE(ong_profiles.verified_at, now()),
              area_type = EXCLUDED.area_type,
              contact_phone = EXCLUDED.contact_phone,
              cep = EXCLUDED.cep,
              street = EXCLUDED.street,
              number = EXCLUDED.number,
              neighborhood = EXCLUDED.neighborhood,
              foundation_year = EXCLUDED.foundation_year,
              verification_status = 'APPROVED',
              verification_reviewed_at = now()
            "#,
        )
        .bind(Uuid::parse_str(ong.id)?)
        .bind(Uuid::parse_str(ong.user_id)?)
        .bind(ong.legal_name)
        .bind(ong.cnpj)
        .bind(ong.mission)
        .bind(ong.city)
        .bind(ong.state)
        .bind(ong.area_type)
        .bind(ong.contact_phone)
        .bind(ong.cep)
        .bind(ong.street)
        .bind(ong.number)
        .bind(ong.neighborhood)
        .bind(ong.foundation_year)
        .execute(pool)
        .await
        .with_context(|| format!("failed to upsert ONG {}", ong.legal_name))?;
    }

    Ok(())
}

fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("password hashing failed: {error}"))
}

async fn print_summary(pool: &PgPool) -> anyhow::Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT account_type::text AS account_type, count(*)::bigint AS total
        FROM users
        WHERE email = ANY($1)
        GROUP BY account_type
        ORDER BY account_type
        "#,
    )
    .bind(seed_emails())
    .fetch_all(pool)
    .await?;

    for row in rows {
        let account_type: String = row.get("account_type");
        let total: i64 = row.get("total");
        println!("{account_type}: {total}");
    }

    let approved_ongs: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*)::bigint
        FROM ong_profiles op
        JOIN users u ON u.id = op.user_id
        WHERE u.email = ANY($1) AND op.verification_status = 'APPROVED'
        "#,
    )
    .bind(seed_emails())
    .fetch_one(pool)
    .await?;
    println!("approved_ongs: {approved_ongs}");

    Ok(())
}

fn seed_emails() -> Vec<&'static str> {
    users().into_iter().map(|user| user.email).collect()
}

fn users() -> Vec<SeedUser> {
    vec![
        SeedUser {
            id: "11111111-1111-7111-8111-111111111111",
            name: "Paulo Admin",
            email: "paulo@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/32.jpg",
            account_type: "admin",
            verified: true,
            trust_score: 100,
            gender: Some("male"),
            cep: "01001000",
            street: "Praca da Se",
            number: "100",
            neighborhood: "Se",
            city: "Sao Paulo",
            state: "SP",
            password: ADMIN_PASSWORD,
        },
        SeedUser {
            id: "22222222-2222-7222-8222-222222222221",
            name: "Instituto Patas do Bem",
            email: "ong.patas@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/women/68.jpg",
            account_type: "ong",
            verified: true,
            trust_score: 95,
            gender: None,
            cep: "04094050",
            street: "Avenida Conselheiro Rodrigues Alves",
            number: "455",
            neighborhood: "Vila Mariana",
            city: "Sao Paulo",
            state: "SP",
            password: ONG_PASSWORD,
        },
        SeedUser {
            id: "22222222-2222-7222-8222-222222222222",
            name: "Resgate Animal Esperanca",
            email: "ong.esperanca@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/76.jpg",
            account_type: "ong",
            verified: true,
            trust_score: 93,
            gender: None,
            cep: "05422000",
            street: "Rua dos Pinheiros",
            number: "870",
            neighborhood: "Pinheiros",
            city: "Sao Paulo",
            state: "SP",
            password: ONG_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333331",
            name: "Ana Ribeiro",
            email: "ana.ribeiro@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/women/12.jpg",
            account_type: "person",
            verified: true,
            trust_score: 70,
            gender: Some("female"),
            cep: "01311000",
            street: "Avenida Paulista",
            number: "900",
            neighborhood: "Bela Vista",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333332",
            name: "Bruno Almeida",
            email: "bruno.almeida@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/11.jpg",
            account_type: "person",
            verified: true,
            trust_score: 66,
            gender: Some("male"),
            cep: "01415001",
            street: "Rua Augusta",
            number: "1410",
            neighborhood: "Consolacao",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333333",
            name: "Carla Mendes",
            email: "carla.mendes@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/women/45.jpg",
            account_type: "person",
            verified: true,
            trust_score: 72,
            gender: Some("female"),
            cep: "04543011",
            street: "Rua Olimpiadas",
            number: "205",
            neighborhood: "Vila Olimpia",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333334",
            name: "Diego Santos",
            email: "diego.santos@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/41.jpg",
            account_type: "person",
            verified: true,
            trust_score: 64,
            gender: Some("male"),
            cep: "05010000",
            street: "Rua Turiassu",
            number: "720",
            neighborhood: "Perdizes",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333335",
            name: "Elisa Costa",
            email: "elisa.costa@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/women/22.jpg",
            account_type: "person",
            verified: true,
            trust_score: 69,
            gender: Some("female"),
            cep: "02012010",
            street: "Rua Voluntarios da Patria",
            number: "1188",
            neighborhood: "Santana",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333336",
            name: "Felipe Nogueira",
            email: "felipe.nogueira@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/52.jpg",
            account_type: "person",
            verified: true,
            trust_score: 67,
            gender: Some("male"),
            cep: "03062000",
            street: "Rua Itapura",
            number: "310",
            neighborhood: "Tatuape",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333337",
            name: "Gabriela Torres",
            email: "gabriela.torres@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/women/65.jpg",
            account_type: "person",
            verified: true,
            trust_score: 71,
            gender: Some("female"),
            cep: "04101000",
            street: "Rua Domingos de Morais",
            number: "1550",
            neighborhood: "Vila Mariana",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333338",
            name: "Henrique Rocha",
            email: "henrique.rocha@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/67.jpg",
            account_type: "person",
            verified: true,
            trust_score: 68,
            gender: Some("male"),
            cep: "05652000",
            street: "Avenida Giovanni Gronchi",
            number: "3210",
            neighborhood: "Morumbi",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333339",
            name: "Isabela Martins",
            email: "isabela.martins@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/women/74.jpg",
            account_type: "person",
            verified: true,
            trust_score: 73,
            gender: Some("female"),
            cep: "03318000",
            street: "Rua Serra de Jurea",
            number: "512",
            neighborhood: "Tatuape",
            city: "Sao Paulo",
            state: "SP",
            password: USER_PASSWORD,
        },
        SeedUser {
            id: "33333333-3333-7333-8333-333333333340",
            name: "Joao Pereira",
            email: "joao.pereira@helpin.com",
            avatar_url: "https://randomuser.me/api/portraits/men/84.jpg",
            account_type: "person",
            verified: true,
            trust_score: 65,
            gender: Some("male"),
            cep: "06020010",
            street: "Avenida dos Autonomistas",
            number: "2200",
            neighborhood: "Centro",
            city: "Osasco",
            state: "SP",
            password: USER_PASSWORD,
        },
    ]
}

fn ongs() -> Vec<SeedOng> {
    vec![
        SeedOng {
            id: "44444444-4444-7444-8444-444444444441",
            user_id: "22222222-2222-7222-8222-222222222221",
            legal_name: "Instituto Patas do Bem",
            cnpj: "11222333000181",
            mission: "Resgate, tratamento veterinario e adocao responsavel de animais em risco.",
            city: "Sao Paulo",
            state: "SP",
            area_type: "rescue",
            contact_phone: "(11) 99111-0001",
            cep: "04094050",
            street: "Avenida Conselheiro Rodrigues Alves",
            number: "455",
            neighborhood: "Vila Mariana",
            foundation_year: 2017,
        },
        SeedOng {
            id: "44444444-4444-7444-8444-444444444442",
            user_id: "22222222-2222-7222-8222-222222222222",
            legal_name: "Resgate Animal Esperanca",
            cnpj: "22333444000172",
            mission: "Atendimento emergencial, lares temporarios e campanhas de castracao.",
            city: "Sao Paulo",
            state: "SP",
            area_type: "rescue",
            contact_phone: "(11) 99222-0002",
            cep: "05422000",
            street: "Rua dos Pinheiros",
            number: "870",
            neighborhood: "Pinheiros",
            foundation_year: 2015,
        },
    ]
}
