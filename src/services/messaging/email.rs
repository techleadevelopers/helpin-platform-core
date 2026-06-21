use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, Message, SmtpTransport,
    Transport,
};

use crate::config::Config;

#[derive(Clone, Debug)]
pub struct EmailService {
    config: Config,
}

#[derive(Clone, Debug)]
pub struct OutboundEmail {
    pub to_email: String,
    pub to_name: Option<String>,
    pub subject: String,
    pub text_body: String,
}

impl EmailService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn is_configured(&self) -> bool {
        self.config.smtp_host.is_some()
            && self.config.smtp_user.is_some()
            && self.config.smtp_pass.is_some()
    }

    pub async fn send(&self, email: OutboundEmail) -> anyhow::Result<()> {
        if !self.is_configured() {
            tracing::warn!(
                to = %email.to_email,
                subject = %email.subject,
                "SMTP is not configured; email was not sent"
            );
            return Ok(());
        }

        let config = self.config.clone();
        tokio::task::spawn_blocking(move || send_blocking(&config, email)).await?
    }

    pub async fn send_password_reset(&self, to_email: &str, token: &str) -> anyhow::Result<()> {
        let reset_url = format!(
            "{}/reset-password?token={}",
            self.config.app_public_url.trim_end_matches('/'),
            token
        );
        self.send(OutboundEmail {
            to_email: to_email.to_string(),
            to_name: None,
            subject: "Recuperação de senha Helpin".to_string(),
            text_body: format!(
                "Recebemos uma solicitação para recuperar sua senha no Helpin.\n\nUse este link para continuar:\n{reset_url}\n\nSe voce nao pediu isso, ignore esta mensagem."
            ),
        })
        .await
    }
}

fn send_blocking(config: &Config, email: OutboundEmail) -> anyhow::Result<()> {
    let host = config
        .smtp_host
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("SMTP_HOST is missing"))?;
    let user = config
        .smtp_user
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("SMTP_USER is missing"))?;
    let pass = config
        .smtp_pass
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("SMTP_PASS is missing"))?;

    let from: Mailbox =
        format!("{} <{}>", config.smtp_from_name, config.smtp_from_email).parse()?;
    let to: Mailbox = match email.to_name {
        Some(name) if !name.trim().is_empty() => format!("{name} <{}>", email.to_email).parse()?,
        _ => email.to_email.parse()?,
    };

    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(email.subject)
        .body(email.text_body)?;

    let credentials = Credentials::new(user.clone(), pass.clone());
    let transport = if config.smtp_secure {
        SmtpTransport::relay(host)?
            .port(config.smtp_port)
            .credentials(credentials)
            .build()
    } else {
        SmtpTransport::builder_dangerous(host)
            .port(config.smtp_port)
            .credentials(credentials)
            .build()
    };

    transport.send(&message)?;
    Ok(())
}
