//! Email notification service — send notification emails via SMTP.
//!
//! Configuration is provided via CLI flags:
//! - `--smtp-host` — SMTP server hostname
//! - `--smtp-port` — SMTP server port (default 587)
//! - `--smtp-user` — SMTP username
//! - `--smtp-pass` — SMTP password
//! - `--smtp-from` — From email address
//!
//! If SMTP is not configured, email sending is silently skipped.

use crate::error::{CoreContext, CoreResult};
use lettre::{
    message::header::ContentType, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use serde::{Deserialize, Serialize};

/// SMTP configuration for sending emails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub from: String,
}

impl SmtpConfig {
    /// Create a new SMTP config.
    pub fn new(host: &str, port: u16, user: &str, pass: &str, from: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            user: user.to_string(),
            pass: pass.to_string(),
            from: from.to_string(),
        }
    }
}

/// Send a notification email.
pub async fn send_notification_email(
    config: &SmtpConfig,
    to: &str,
    subject: &str,
    body: &str,
) -> CoreResult<()> {
    let email = Message::builder()
        .from(config.from.parse().context("invalid from address")?)
        .to(to.parse().context("invalid to address")?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body.to_string())
        .context("failed to build email message")?;

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
        .context("failed to create SMTP relay")?
        .port(config.port)
        .credentials(lettre::transport::smtp::authentication::Credentials::new(
            config.user.clone(),
            config.pass.clone(),
        ))
        .timeout(Some(std::time::Duration::from_secs(30)))
        .build();

    match mailer.send(email).await {
        Ok(_) => {
            tracing::info!(to = %to, subject = %subject, "notification email sent");
            Ok(())
        }
        Err(e) => {
            tracing::warn!(to = %to, subject = %subject, error = %e, "failed to send notification email");
            Err(crate::error::CoreError::internal(format!(
                "SMTP send failed: {}",
                e
            )))
        }
    }
}

/// Send a simple HTML notification email with a styled template.
pub async fn send_html_notification(
    config: &SmtpConfig,
    to: &str,
    title: &str,
    message: &str,
    action_url: Option<&str>,
) -> CoreResult<()> {
    let html_body = render_html(title, message, action_url);
    send_notification_email(config, to, title, &html_body).await
}

/// Render the shared notification template.
///
/// `title` and `message` are escaped: both carry user-authored text (issue
/// titles, comment bodies), so the template must never interpolate them raw.
/// Callers express line breaks as `\n` in `message`, not as `<br/>` — the
/// paragraph is `white-space: pre-wrap`, so newlines survive and markup does
/// not (#19 R19-3).
fn render_html(title: &str, message: &str, action_url: Option<&str>) -> String {
    let action_html = match action_url {
        Some(url) => format!(
            r#"<div style="margin-top: 16px;"><a href="{}" style="background: #4f46e5; color: white; padding: 10px 20px; border-radius: 6px; text-decoration: none; display: inline-block;">View Details</a></div>"#,
            url
        ),
        None => String::new(),
    };

    format!(
        r#"<html><body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #f6f8fa; margin: 0; padding: 20px;">
<div style="max-width: 600px; margin: 0 auto; background: white; border-radius: 8px; padding: 24px; box-shadow: 0 1px 3px rgba(0,0,0,0.1);">
  <div style="border-bottom: 2px solid #4f46e5; padding-bottom: 12px; margin-bottom: 16px;">
    <h2 style="margin: 0; color: #1f2937;">{title}</h2>
  </div>
  <p style="color: #4b5563; line-height: 1.6; white-space: pre-wrap;">{message}</p>
  {action_html}
  <hr style="border: none; border-top: 1px solid #e5e7eb; margin: 20px 0;" />
  <p style="color: #9ca3af; font-size: 12px;">You received this email because an action on IronForge involved your account.</p>
</div>
</body></html>"#,
        title = html_escape(title),
        message = html_escape(message),
        action_html = action_html,
    )
}

/// Simple HTML entity escaping.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::render_html;

    /// The footer used to promise "you have notifications enabled". This
    /// template is shared with mail that no notification preference governs
    /// (password resets), so the sentence spoke for a switch the recipient
    /// never agreed to here (#19 R19-3).
    #[test]
    fn the_footer_does_not_claim_a_notification_switch() {
        let html = render_html("Title", "Body", None);
        assert!(
            !html.contains("notifications enabled"),
            "footer still claims a user-configurable notification switch: {html}"
        );
        assert!(
            !html.to_lowercase().contains("preferences"),
            "footer still points at a preferences screen: {html}"
        );
    }

    /// Callers pass newlines, not `<br/>`: the paragraph is `pre-wrap`, so the
    /// newlines render, and escaping leaves no literal `&lt;br` behind.
    #[test]
    fn newlines_survive_without_html_markup() {
        let html = render_html("CI", "line one\n\nline two", None);
        assert!(
            html.contains("white-space: pre-wrap"),
            "the message paragraph must preserve newlines: {html}"
        );
        assert!(
            html.contains("line one\n\nline two"),
            "newlines were collapsed instead of preserved: {html}"
        );
        assert!(
            !html.contains("&lt;br"),
            "the body must not carry escaped markup: {html}"
        );
    }

    /// Escaping is the XSS guard and must stay: a body that looks like markup
    /// is rendered as text.
    #[test]
    fn user_text_is_escaped() {
        let html = render_html("T", "<script>alert(1)</script>", None);
        assert!(
            !html.contains("<script>"),
            "user text reached the template unescaped: {html}"
        );
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "user text must be escaped, not dropped: {html}"
        );
    }

    /// Titles are user-authored too (issue titles, PR titles).
    #[test]
    fn titles_are_escaped() {
        let html = render_html("<img src=x onerror=alert(1)>", "b", None);
        assert!(
            !html.contains("<img src=x"),
            "title was not escaped: {html}"
        );
    }

    /// The action button only appears when a URL is given.
    #[test]
    fn the_action_button_is_conditional() {
        assert!(render_html("T", "b", Some("https://example.com/x")).contains("View Details"));
        assert!(!render_html("T", "b", None).contains("View Details"));
    }
}
