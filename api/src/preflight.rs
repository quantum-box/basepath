//! Redacted configuration and reachability checks for a live Tachyon / Field setup.
use crate::{
    auth::{AuthConfig, TachyonAuth},
    field::FieldClient,
    model::{ApiError, Result},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PreflightCheck {
    pub name: &'static str,
    pub status: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PreflightReport {
    pub mode: String,
    pub configuration_ready: bool,
    pub requires_authenticated_check: bool,
    pub checks: Vec<PreflightCheck>,
}

impl PreflightReport {
    pub fn succeeded(&self) -> bool {
        self.configuration_ready
    }
}

fn check(
    checks: &mut Vec<PreflightCheck>,
    name: &'static str,
    status: &'static str,
    message: impl Into<String>,
) {
    checks.push(PreflightCheck {
        name,
        status,
        message: message.into(),
    });
}

fn missing_environment_variables(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .filter(|name| {
            std::env::var(name)
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        })
        .map(|name| (*name).to_owned())
        .collect()
}

fn classify_auth_boundary(
    checks: &mut Vec<PreflightCheck>,
    name: &'static str,
    label: &str,
    status: u16,
) {
    match status {
        401 | 403 => check(
            checks,
            name,
            "ok",
            format!("{label}へ到達し、未認証要求が拒否されました"),
        ),
        400 | 422 => check(
            checks,
            name,
            "warning",
            format!("{label}へ到達しましたが、未認証要求への応答はHTTP {status}でした"),
        ),
        405 => check(
            checks,
            name,
            "error",
            format!("{label}が必要なHTTPメソッドを受け付けません"),
        ),
        200..=299 => check(
            checks,
            name,
            "error",
            format!("{label}が未認証の確認要求を受理しました"),
        ),
        404 => check(
            checks,
            name,
            "error",
            format!("{label}のパスが見つかりません"),
        ),
        _ => check(
            checks,
            name,
            "error",
            format!("{label}がHTTP {status}を返しました"),
        ),
    }
}

pub async fn run(
    mode: String,
    auth_config: Result<AuthConfig>,
    client_secret_configured: bool,
    field_config: Result<Option<FieldClient>>,
) -> PreflightReport {
    let mut checks = Vec::new();
    if mode == "tachyon" {
        check(
            &mut checks,
            "mode",
            "ok",
            "Tachyon認証モードが選択されています",
        );
    } else {
        check(
            &mut checks,
            "mode",
            "error",
            "PATHBASE_MODE=tachyon を設定してください",
        );
    }

    check(
        &mut checks,
        "oidc_client_auth",
        if client_secret_configured {
            "ok"
        } else {
            "warning"
        },
        if client_secret_configured {
            "OIDCクライアントシークレットが設定されています"
        } else {
            "クライアントシークレット未設定のため、PKCEを使う公開クライアントとして確認します"
        },
    );

    match auth_config {
        Ok(config) => {
            check(
                &mut checks,
                "tachyon_configuration",
                "ok",
                "公開URL、コールバックURL、OIDC、Tachyon APIの設定形式は有効です",
            );
            match TachyonAuth::new(config).await {
                Ok(auth) => {
                    check(
                        &mut checks,
                        "oidc_discovery",
                        "ok",
                        "OIDC Discoveryを取得し、issuerと各エンドポイントを検証しました",
                    );
                    match auth.probe_verification_boundary().await {
                        Ok(status) => classify_auth_boundary(
                            &mut checks,
                            "tachyon_verify",
                            "Tachyonのトークン検証API",
                            status,
                        ),
                        Err(error) => check(&mut checks, "tachyon_verify", "error", error.message),
                    }
                }
                Err(error) => check(&mut checks, "oidc_discovery", "error", error.message),
            }
        }
        Err(error) => check(&mut checks, "tachyon_configuration", "error", error.message),
    }

    match field_config {
        Ok(Some(field)) => {
            check(
                &mut checks,
                "field_configuration",
                "ok",
                "Field APIとテナント文脈の設定形式は有効です",
            );
            match field.probe_auth_boundary().await {
                Ok(status) => classify_auth_boundary(
                    &mut checks,
                    "field_boundary",
                    "Fieldの権限付きテナント一覧API",
                    status,
                ),
                Err(error) => check(&mut checks, "field_boundary", "error", error.message),
            }
        }
        Ok(None) => check(
            &mut checks,
            "field_configuration",
            "error",
            "FIELD_API_URL、FIELD_PLATFORM_ID、FIELD_OPERATOR_IDを設定してください",
        ),
        Err(error) => check(&mut checks, "field_configuration", "error", error.message),
    }

    let configuration_ready =
        mode == "tachyon" && !checks.iter().any(|entry| entry.status == "error");
    PreflightReport {
        mode,
        configuration_ready,
        requires_authenticated_check: true,
        checks,
    }
}

pub async fn run_from_env() -> PreflightReport {
    let mode = std::env::var("PATHBASE_MODE").unwrap_or_default();
    let client_secret_configured =
        std::env::var("TACHYON_OIDC_CLIENT_SECRET").is_ok_and(|value| !value.trim().is_empty());
    let missing_auth = missing_environment_variables(&[
        "PATHBASE_PUBLIC_URL",
        "TACHYON_OIDC_ISSUER",
        "TACHYON_OIDC_CLIENT_ID",
        "TACHYON_OIDC_REDIRECT_URI",
        "TACHYON_API_URL",
        "PATHBASE_SESSION_KEYS",
    ]);
    let auth_config = if missing_auth.is_empty() {
        AuthConfig::from_env().and_then(|config| {
            crate::auth::session_keys_from_env()?;
            Ok(config)
        })
    } else {
        Err(ApiError::invalid(&format!(
            "設定されていません: {}",
            missing_auth.join(", ")
        )))
    };
    let missing_field =
        missing_environment_variables(&["FIELD_API_URL", "FIELD_PLATFORM_ID", "FIELD_OPERATOR_ID"]);
    let field_config = if missing_field.is_empty() {
        FieldClient::from_env()
    } else {
        Err(ApiError::invalid(&format!(
            "設定されていません: {}",
            missing_field.join(", ")
        )))
    };
    run(mode, auth_config, client_secret_configured, field_config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_not_allowed_is_a_preflight_error() {
        let mut checks = Vec::new();
        classify_auth_boundary(&mut checks, "boundary", "Upstream", 405);
        assert_eq!(checks[0].status, "error");
    }
}
