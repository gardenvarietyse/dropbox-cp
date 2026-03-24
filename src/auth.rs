//! Resolve Dropbox credentials from the environment.

use dropbox_sdk::default_client::{NoauthDefaultClient, UserAuthDefaultClient};
use dropbox_sdk::oauth2::Authorization;

const DROPBOX_ACCESS_TOKEN: &str = "DROPBOX_ACCESS_TOKEN";
const DROPBOX_APP_KEY: &str = "DROPBOX_APP_KEY";
const DROPBOX_APP_SECRET: &str = "DROPBOX_APP_SECRET";
const DROPBOX_REFRESH_TOKEN: &str = "DROPBOX_REFRESH_TOKEN";

/// Build a Dropbox API client from environment variables, or print a detailed error.
pub fn client_from_env() -> Result<UserAuthDefaultClient, String> {
    let access = trimmed_env(DROPBOX_ACCESS_TOKEN);
    let key = trimmed_env(DROPBOX_APP_KEY);
    let secret = trimmed_env(DROPBOX_APP_SECRET);
    let refresh = trimmed_env(DROPBOX_REFRESH_TOKEN);

    if !access.is_empty() {
        #[allow(deprecated)]
        let auth = Authorization::from_long_lived_access_token(access);
        return Ok(UserAuthDefaultClient::new(auth));
    }

    let have_full_refresh = !key.is_empty() && !secret.is_empty() && !refresh.is_empty();
    let any_refresh = !key.is_empty() || !secret.is_empty() || !refresh.is_empty();

    if have_full_refresh {
        let mut auth =
            Authorization::from_client_secret_refresh_token(key, secret, refresh);
        auth
            .obtain_access_token(NoauthDefaultClient::default())
            .map_err(|e| format!("Failed to refresh Dropbox access token: {e}"))?;
        return Ok(UserAuthDefaultClient::new(auth));
    }

    if any_refresh {
        let mut lines = String::from(
            "Incomplete Dropbox credentials for refresh-token authentication. Each variable below must be set (non-empty):\n",
        );
        for name in [
            DROPBOX_APP_KEY,
            DROPBOX_APP_SECRET,
            DROPBOX_REFRESH_TOKEN,
        ] {
            let present = !trimmed_env(name).is_empty();
            let desc = match name {
                DROPBOX_APP_KEY => "Dropbox app key (OAuth client_id) from the App Console.",
                DROPBOX_APP_SECRET => "Dropbox app secret (OAuth client_secret) from the App Console.",
                DROPBOX_REFRESH_TOKEN => "OAuth2 refresh token for the user you want to upload as.",
                _ => "",
            };
            lines.push_str(&format!(
                "  {name}  {}{desc}\n",
                if present { "(set) " } else { "— MISSING — " }
            ));
        }
        return Err(lines);
    }

    Err(format!(
        "Missing Dropbox credentials. Use one of the following:\n\
         \n\
         Option A — access token (short-lived unless you refresh it yourself):\n\
           {DROPBOX_ACCESS_TOKEN}   OAuth2 access token for your Dropbox app (Bearer token).\n\
         \n\
         Option B — app + refresh token (recommended for automation):\n\
           {DROPBOX_APP_KEY}          Your app's App key (OAuth client_id).\n\
           {DROPBOX_APP_SECRET}      Your app's App secret (OAuth client_secret).\n\
           {DROPBOX_REFRESH_TOKEN}   A refresh token for the linked user account.\n\
         \n\
         Obtain tokens via the Dropbox App Console and OAuth 2 guide:\n\
         https://www.dropbox.com/developers/documentation/http/documentation#authorization\n\
         \n\
         Currently missing:\n\
           {DROPBOX_ACCESS_TOKEN} (for option A), or all of:\n\
           {DROPBOX_APP_KEY}, {DROPBOX_APP_SECRET}, {DROPBOX_REFRESH_TOKEN} (for option B)."
    ))
}

fn trimmed_env(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_default()
        .trim()
        .to_owned()
}
