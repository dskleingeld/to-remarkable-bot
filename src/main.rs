use reqwest;
use snafu::Snafu;

mod api;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Could not reach remarkable cloud"))]
    RemarkableApiUnreachable {
        source: reqwest::Error,
    },
    #[snafu(display("Could not load a previous token"))]
    TokenLoadError {
        source: std::io::Error,
    },
    #[snafu(display("Could not save new token"))]
    TokenStoreError {
        source: std::io::Error,
    },
    #[snafu(display("remarkable api send us an error"))]
    RemarkableApiError {
        status: reqwest::StatusCode,
    },
    #[snafu(display("remarkable api send an ill shaped response that cant be parsed"))]
    IncorrectApiResponse {
        source: serde_json::error::Error,
        response: String,
    },
    #[snafu(display("error occured during zipping of pdf and metadata"))]
    ZipError {
        source: zip::result::ZipError,
    }
}

fn upload_file() -> Result<(), Error>{
    let token = api::load_token();
    let token = if token.is_err(){
        api::get_new_token()?
    } else {
        api::refresh_token(&token.unwrap())?
    };
    
    let storage_url = api::locate_storage_api(&token)?;
    api::upload_pdf(token, vec!(0;1), storage_url, String::from("testfile"))?;

    Ok(())
}

fn main() {

    upload_file().unwrap();
    println!("Hello, world!");
}
