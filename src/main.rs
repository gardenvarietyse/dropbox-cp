//! `dcp` — copy local files or directories into Dropbox.

mod auth;
mod upload;

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

const ABOUT: &str = r#"Copy a local file or directory tree into a path in the connected Dropbox account.

Valid usage:

  dcp [OPTIONS] <SOURCE> <DEST>

  Single file — upload one file to an exact Dropbox path (DEST includes the file name):

    dcp ./notes.txt /backup/notes.txt

  Directory — copy the contents of SOURCE into the Dropbox folder DEST (DEST is the folder root; relative paths are preserved). Requires -r:

    dcp -r ./stuff /backup/stuff

DEST is a path in Dropbox. A leading '/' is optional; relative DEST values are treated as starting at the account root (e.g. backup/x becomes /backup/x).

Environment (one of two options):

  Option A — access token:
    DROPBOX_ACCESS_TOKEN   Bearer token for the Dropbox app (short-lived unless you refresh it yourself).

  Option B — refresh token (recommended for automation):
    DROPBOX_APP_KEY        App key from the Dropbox App Console (OAuth client_id).
    DROPBOX_APP_SECRET     App secret (OAuth client_secret).
    DROPBOX_REFRESH_TOKEN  Refresh token for the user account to upload into.

See: https://www.dropbox.com/developers/documentation/http/documentation#authorization
"#;

#[derive(Parser)]
#[command(name = "dcp")]
#[command(version)]
#[command(about = ABOUT, long_about = ABOUT)]
struct Cli {
    /// Recursively copy a directory (required when SOURCE is a directory).
    #[arg(short = 'r', long)]
    recursive: bool,

    /// Overwrite files that already exist at the destination path in Dropbox.
    #[arg(short = 'f', long)]
    force: bool,

    /// Local file or directory to upload.
    source: PathBuf,

    /// Destination path in Dropbox (e.g. /backup/stuff or backup/stuff).
    dest: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let client = match auth::client_from_env() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(1);
        }
    };

    let failed = upload::copy_to_dropbox(
        &client,
        &cli.source,
        &cli.dest,
        cli.recursive,
        cli.force,
    );

    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
