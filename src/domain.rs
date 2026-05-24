#[cfg(test)]
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

#[cfg(test)]
static AUTHORS: LazyLock<Vec<Author>> = LazyLock::new(build_authors);
#[cfg(test)]
#[allow(dead_code)]
static ONGS: LazyLock<Vec<Ong>> = LazyLock::new(build_ongs);
#[cfg(test)]
static POSTS: LazyLock<Vec<Post>> = LazyLock::new(build_posts);
#[cfg(test)]
#[allow(dead_code)]
static CONVERSATIONS: LazyLock<Vec<ChatConversation>> = LazyLock::new(build_conversations);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Person,
    Ong,
    Vet,
    Admin,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AnimalType {
    Dog,
    Cat,
    Other,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PostType {
    Adoption,
    Lost,
    Found,
    Emergency,
    Campaign,
    Post,
}

#[derive(Clone, Debug, Serialize)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    pub verified: bool,
    #[serde(rename = "type")]
    pub account_type: AccountType,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostMedia {
    pub id: String,
    pub url: String,
    pub content_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<u64>,
    pub moderation_status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: String,
    #[serde(rename = "type")]
    pub post_type: PostType,
    pub animal_type: AnimalType,
    pub name: String,
    pub breed: String,
    pub age: String,
    pub description: String,
    pub location: String,
    pub neighborhood: String,
    pub image: Option<String>,
    pub images: Vec<PostMedia>,
    pub text_only: bool,
    pub author: Author,
    pub likes: u32,
    pub comments: u32,
    pub shares: u32,
    pub urgent: bool,
    pub rescue_status: String,
    pub resolved_at: Option<String>,
    pub created_at: String,
    pub contact: String,
    pub tags: Vec<String>,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ong {
    pub id: String,
    pub name: String,
    pub email: String,
    pub short_name: String,
    pub avatar_url: Option<String>,
    pub description: String,
    pub mission: String,
    pub location: String,
    pub city: String,
    pub state: String,
    pub verified: bool,
    pub animals_rescued: u32,
    pub active_cases: u32,
    pub adoptions: u32,
    pub animal_types: Vec<String>,
    pub followers: u32,
    pub since: String,
    pub cnpj: String,
    pub contact: String,
    pub cause: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversation {
    pub id: String,
    pub post_id: String,
    pub participant: Author,
    pub last_message: String,
    pub last_message_time: String,
    pub unread: u32,
    pub post_title: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub sender_id: String,
    pub body: String,
    pub created_at: String,
}

#[cfg(test)]
pub fn seed_authors() -> Vec<Author> {
    AUTHORS.clone()
}

#[cfg(test)]
#[allow(dead_code)]
pub fn seed_ongs() -> Vec<Ong> {
    ONGS.clone()
}

#[cfg(test)]
pub fn seed_posts() -> Vec<Post> {
    POSTS.clone()
}

#[cfg(test)]
#[allow(dead_code)]
pub fn seed_conversations() -> Vec<ChatConversation> {
    CONVERSATIONS.clone()
}

#[cfg(test)]
fn build_authors() -> Vec<Author> {
    vec![
        author("u1", "Instituto Amigos dos Animais", true, AccountType::Ong),
        author("u2", "Protetora Fernanda Lima", true, AccountType::Person),
        author("u3", "ONG Patinhas Felizes", true, AccountType::Ong),
        author("u4", "Dr. Carlos Veterinário", true, AccountType::Vet),
        author("u5", "Mariana Santos", false, AccountType::Person),
        author("u6", "Abrigo Municipal SP", true, AccountType::Ong),
    ]
}

#[cfg(test)]
#[allow(dead_code)]
fn build_ongs() -> Vec<Ong> {
    vec![
        ong(
            "o1",
            "Instituto Amigos dos Animais",
            "IAA",
            "Maior rede de proteção animal do Brasil, com atuação nacional.",
            "Vila Mariana, São Paulo, SP",
            "São Paulo",
            "SP",
            15240,
            87,
            9430,
            "(11) 99999-0001",
            "Adoção responsável e resgate urbano",
        ),
        ong(
            "o2",
            "ONG Patinhas Felizes",
            "Patinhas",
            "Especializada em resgate e reabilitação de animais ví­timas de maus-tratos.",
            "Cambuí­, Campinas, SP",
            "Campinas",
            "SP",
            4780,
            34,
            3210,
            "(19) 98888-0002",
            "Combate a maus-tratos e adoção",
        ),
        ong(
            "o3",
            "Abrigo Municipal SP",
            "AMSP",
            "Abrigo píºblico com suporte veterinário completo.",
            "Mooca, São Paulo, SP",
            "São Paulo",
            "SP",
            32100,
            198,
            27600,
            "(11) 97777-0003",
            "Bem-estar animal píºblico",
        ),
        ong(
            "o4",
            "Clí­nica Vet Solidária",
            "VetSol",
            "Rede de clí­nicas que atende animais de rua e famí­lias vulneráveis.",
            "Lapa, São Paulo, SP",
            "São Paulo",
            "SP",
            8900,
            55,
            1200,
            "(11) 96666-0004",
            "Saíºde e bem-estar veterinário",
        ),
        ong(
            "o5",
            "Resgate Animal Brasil",
            "RAB",
            "Atua em emergências e desastres naturais.",
            "Centro, Rio de Janeiro, RJ",
            "Rio de Janeiro",
            "RJ",
            6200,
            22,
            4100,
            "(21) 95555-0005",
            "Resgate em emergências e desastres",
        ),
        ong(
            "o6",
            "Fundo Animal BR",
            "FABR",
            "Conecta doadores a ONGs verificadas com transparência.",
            "Pinheiros, São Paulo, SP",
            "São Paulo",
            "SP",
            0,
            0,
            0,
            "(11) 94444-0006",
            "Captação e distribuição de recursos",
        ),
    ]
}

#[cfg(test)]
fn build_posts() -> Vec<Post> {
    let authors = seed_authors();
    vec![
        post("1", PostType::Adoption, AnimalType::Dog, "Mel", "Vira-lata Caramelo", "2 anos", "Mel í© dí³cil, vacinada, castrada e busca um lar cheio de amor.", "São Paulo, SP", "Vila Mariana", authors[0].clone(), 127, 23, 45, false, "2h atrás", "(11) 99999-0001", &["vacinada", "castrada", "dí³cil"], -23.5898, -46.6348),
        post("2", PostType::Emergency, AnimalType::Cat, "Sem nome", "Gatinho tigrado", "Estimado 3 meses", "Gatinho encontrado ferido na Av. Paulista. Precisa de atendimento veterinário urgente.", "São Paulo, SP", "Bela Vista", authors[1].clone(), 340, 67, 210, true, "45min atrás", "(11) 98888-0002", &["emergência", "ferido"], -23.5614, -46.6559),
        post("3", PostType::Lost, AnimalType::Dog, "Thor", "Golden Retriever", "4 anos", "Thor fugiu no Parque Ibirapuera. Tem coleira azul e microchip.", "São Paulo, SP", "Ibirapuera", authors[4].clone(), 892, 134, 567, true, "1 dia atrás", "(11) 97777-0003", &["perdido", "recompensa"], -23.5874, -46.6576),
        post("4", PostType::Found, AnimalType::Cat, "Desconhecido", "Siamês", "Adulto", "Gato siamês encontrado no Jardins, seguro e bem alimentado.", "São Paulo, SP", "Jardins", authors[4].clone(), 56, 12, 34, false, "3h atrás", "(11) 96666-0004", &["encontrado", "siamês"], -23.5674, -46.6694),
        post("5", PostType::Adoption, AnimalType::Dog, "Pipoca", "Poodle mix", "1 ano", "Pipoca í© alegre, brincalhío, vacinado e pronto para um novo lar.", "Campinas, SP", "Cambuí­", authors[2].clone(), 203, 41, 88, false, "5h atrás", "(19) 95555-0005", &["vacinado", "resgatado"], -22.9056, -47.0608),
        post("6", PostType::Campaign, AnimalType::Other, "Campanha Ração Solidária", "Todos os animais", "Vários", "Abrigo com estoque crí­tico de ração para 85 animais.", "São Paulo, SP", "Mooca", authors[5].clone(), 412, 78, 305, false, "1 dia atrás", "(11) 94444-0006", &["campanha", "doação", "ração"], -23.5599, -46.5978),
        post("post1", PostType::Post, AnimalType::Dog, "Dica de hoje", "", "", "Cíes precisam de água fresca disponí­vel o dia todo.", "São Paulo, SP", "Pinheiros", authors[3].clone(), 284, 37, 91, false, "1h atrás", "", &["dica", "saíºde"], -23.5663, -46.7017),
    ]
}

#[cfg(test)]
#[allow(dead_code)]
fn build_conversations() -> Vec<ChatConversation> {
    let authors = seed_authors();
    vec![
        conversation(
            "c1",
            "1",
            authors[0].clone(),
            "Olá! Tenho interesse em adotar a Mel.",
            "14:32",
            2,
            "Mel - Adoção",
        ),
        conversation(
            "c2",
            "3",
            authors[4].clone(),
            "Acho que vi o Thor hoje cedo.",
            "12:15",
            0,
            "Thor - Perdido",
        ),
        conversation(
            "c3",
            "6",
            authors[5].clone(),
            "Quero contribuir com ração. Como faí§o?",
            "Ontem",
            1,
            "Campanha Ração Solidária",
        ),
    ]
}

#[cfg(test)]
#[allow(dead_code)]
pub fn seed_messages(room_id: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            id: format!("{room_id}-m1"),
            sender_id: "u1".into(),
            body: "Olá, obrigado por chamar pelo ZooHelp.".into(),
            created_at: "14:30".into(),
        },
        ChatMessage {
            id: format!("{room_id}-m2"),
            sender_id: "me".into(),
            body: "Quero ajudar neste caso.".into(),
            created_at: "14:32".into(),
        },
    ]
}

#[cfg(test)]
fn author(id: &str, name: &str, verified: bool, account_type: AccountType) -> Author {
    Author {
        id: id.into(),
        name: name.into(),
        avatar: None,
        verified,
        account_type,
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn ong(
    id: &str,
    name: &str,
    short_name: &str,
    description: &str,
    location: &str,
    city: &str,
    state: &str,
    animals_rescued: u32,
    active_cases: u32,
    adoptions: u32,
    contact: &str,
    cause: &str,
) -> Ong {
    Ong {
        id: id.into(),
        name: name.into(),
        email: String::new(),
        short_name: short_name.into(),
        avatar_url: None,
        description: description.into(),
        mission: "Resgatar, tratar e recolocar animais em lares responsáveis.".into(),
        location: location.into(),
        city: city.into(),
        state: state.into(),
        verified: true,
        animals_rescued,
        active_cases,
        adoptions,
        animal_types: vec!["Cachorros".into(), "Gatos".into(), "Outros".into()],
        followers: animals_rescued.saturating_mul(3).max(14_300),
        since: "2016".into(),
        cnpj: "00.000.000/0001-00".into(),
        contact: contact.into(),
        cause: cause.into(),
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn post(
    id: &str,
    post_type: PostType,
    animal_type: AnimalType,
    name: &str,
    breed: &str,
    age: &str,
    description: &str,
    location: &str,
    neighborhood: &str,
    author: Author,
    likes: u32,
    comments: u32,
    shares: u32,
    urgent: bool,
    created_at: &str,
    contact: &str,
    tags: &[&str],
    latitude: f64,
    longitude: f64,
) -> Post {
    Post {
        id: id.into(),
        post_type,
        animal_type,
        name: name.into(),
        breed: breed.into(),
        age: age.into(),
        description: description.into(),
        location: location.into(),
        neighborhood: neighborhood.into(),
        image: None,
        images: Vec::new(),
        text_only: false,
        author,
        likes,
        comments,
        shares,
        urgent,
        rescue_status: if urgent { "active" } else { "open" }.into(),
        resolved_at: None,
        created_at: created_at.into(),
        contact: contact.into(),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
        latitude,
        longitude,
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn conversation(
    id: &str,
    post_id: &str,
    participant: Author,
    last_message: &str,
    last_message_time: &str,
    unread: u32,
    post_title: &str,
) -> ChatConversation {
    ChatConversation {
        id: id.into(),
        post_id: post_id.into(),
        participant,
        last_message: last_message.into(),
        last_message_time: last_message_time.into(),
        unread,
        post_title: post_title.into(),
    }
}
