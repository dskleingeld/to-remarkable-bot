use reqwest;
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use serde_json;
use snafu::{ensure, ResultExt};
use dialoguer::Input;
use std::fs::File;
use std::io::prelude::*;
use chrono::Utc;

use crate::{IncorrectApiResponse, TokenStoreError, RemarkableApiError, 
            RemarkableApiUnreachable, TokenLoadError, ZipError}; 
use crate::Error;

const AUTH_URL: &'static str = "https://my.remarkable.com/token/json/2/device/new";
const AUTH_REFRESH_URL: &'static str = "https://my.remarkable.com/token/json/2/user/new";
const SERVICE_DISCV_API_URL: &str = "https://service-manager-production-dot-remarkable-production.appspot.com/service/json/1/document-storage?environment=production&group=auth0%7C5a68dc51cb30df3877a1d7c4&apiVer=2";

const TOKEN_PATH: &'static str = "remarkable.token";

#[derive(Serialize)]
struct AuthPayload {
    code: String,
    #[serde(rename = "deviceDesc")]
    device_desc: String,
    #[serde(rename = "deviceID")]
    device_id: String,
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn get_new_token() -> Result<String>{
    let letter_code = Input::<String>::new()
        .with_prompt("Enter 8 letter code from remarkable (leave empty to abort)")
        .interact()
        .unwrap();

    if letter_code.len() != 8 {panic!("Invalid length for letter code");}

    let new_token = authenticate(letter_code)?;

    let mut file = File::create(TOKEN_PATH).unwrap();
    file.write_all(&new_token.as_bytes()).context(TokenStoreError)?;
    
    Ok(new_token)
}

fn authenticate(letter_code: String) -> Result<String> {

    let new_uuid = Uuid::new_v4();
    let payload = AuthPayload {
        code: letter_code,
        device_desc: String::from("desktop-windows"),
        device_id: new_uuid.to_string(),
    };

    let client = reqwest::blocking::Client::new();
    let response = client.post(AUTH_URL)
        .body(serde_json::to_string(&payload).unwrap())
        .send().context(RemarkableApiUnreachable)?;

    ensure!(response.status() == reqwest::StatusCode::OK,
            RemarkableApiError {status: response.status()});

    dbg!(&response);
    let new_token = response.text().unwrap();
    Ok(new_token)
}

pub fn refresh_token(token: &str) -> Result<String> {
    let client = reqwest::blocking::Client::new();
    let response = client.post(AUTH_REFRESH_URL)
        .bearer_auth(token)
        .body("")
        .send().context(RemarkableApiUnreachable)?;

    ensure!(response.status() == reqwest::StatusCode::OK,
            RemarkableApiError {status: response.status()});        

    let new_token = response.text().unwrap();
    Ok(new_token)
}

pub fn load_token() -> Result<String> {
    dbg!("loaded token");
    let mut file = File::open(TOKEN_PATH).context(TokenLoadError)?;
    let mut token = String::default();
    file.read_to_string(&mut token).context(TokenLoadError)?;
    Ok(token)
}

pub fn locate_storage_api(token: &str) -> Result<String> {
    let client = reqwest::blocking::Client::new();
    let response = client.post(SERVICE_DISCV_API_URL)
        .bearer_auth(token)
        .body("")
        .send().context(RemarkableApiUnreachable)?;

    ensure!(response.status() == reqwest::StatusCode::OK,
            RemarkableApiError {status: response.status()});    

    Ok(response.text().unwrap())
}

#[derive(Serialize)]
struct UploadRequest {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Type")]
    doctype: String,
    #[serde(rename = "Version")]
    version: u8,
}

#[derive(Serialize)]
struct UploadMetaData {
    #[serde(rename = "ID")]
    ID: String,
    parent: String,
    #[serde(rename = "VissibleName")]
    VissibleName: String,
    #[serde(rename = "ModifiedClient")]
    ModifiedClient: String,
    #[serde(rename = "Type")]
    Type: String,
    #[serde(rename = "Version")]
    Version: u8,
}

#[derive(Serialize)]
struct FileMetaData {
    #[serde(rename = "extraMeatadata")]
    extraMeatadata: Vec<()>,
    #[serde(rename = "fileType")]
    fileType: String,
    #[serde(rename = "lastOpenedPage")]
    lastOpenedPage: u8,
    #[serde(rename = "lineHeight")]
    lineHeight: i8,
    #[serde(rename = "margins")]
    margins: u8,
    #[serde(rename = "textScale")]
    textScale: f32,
    #[serde(rename = "transform")]
    transform: Vec<()>,
}

#[derive(Deserialize)]
struct UploadDirections {
	ID: String,
	Version: u8,
	Message: String,
	Success: bool,
	BlobURLPut: String,
	BlobURLPutExpires: String,
}

pub fn upload_pdf(token: String, file: Vec<u8>, storage_url: String,
    name: String) -> Result<()>{

    let directions = do_upload_request(&storage_url, &token)?;
    do_upload(&directions, &token, file)?;
    update_metadata(storage_url, token, directions, name)?;

    Ok(())
}

fn update_metadata(storage_url: String, token: String,
                   directions: UploadDirections, name: String) -> Result<()>{
    let mut url = storage_url.clone();
    url.push_str("/json/2/upload/request");

    let payload = UploadMetaData {
        ID: Uuid::new_v4().to_string(),
        parent: directions.ID,
        VissibleName: name,
        ModifiedClient: Utc::now().to_rfc3339(),
        Type: String::from("DocumentType"),
        Version: 1,
    };

    let client = reqwest::blocking::Client::new();
    let response = client.put(&url)
        .bearer_auth(&token)    
        .body(serde_json::to_string(&payload).unwrap())
        .send().context(RemarkableApiUnreachable)?;

    ensure!(response.status() == reqwest::StatusCode::OK,
            RemarkableApiError {status: response.status()}); 
            
    Ok(())
}

fn do_upload(directions: &UploadDirections, token: &String, file: Vec<u8>) -> Result<()> {
    
    let buf = Vec::with_capacity(file.len());
    let writer = std::io::Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(writer);
    let options = zip::write::FileOptions::default();

    let mut name = directions.ID.clone();
    name.push_str(".pdf");
    zip.start_file(name, options).context(ZipError)?;
    zip.write(&file).unwrap();

    let mut name = directions.ID.clone();
    name.push_str(".pagedata");
    zip.start_file(name, options).context(ZipError)?;

    let content = FileMetaData {
        extraMeatadata: Vec::new(),
        fileType: String::from("pdf"),
        lastOpenedPage: 0,
        lineHeight: -1,
        margins: 100,
        textScale: 1.0,
        transform: Vec::new(),       
    };
    let content = serde_json::to_vec_pretty(&content).unwrap();

    let mut name = directions.ID.clone();
    name.push_str(".content");
    zip.start_file(name, options).context(ZipError)?;
    zip.write(&content).unwrap();
    let zipped = zip.finish().context(ZipError)?.into_inner();

    let client = reqwest::blocking::Client::new();
    let response = client.put(&directions.BlobURLPut)
        .bearer_auth(&token)
        .body(zipped)
        .send().context(RemarkableApiUnreachable)?;

    ensure!(response.status() == reqwest::StatusCode::OK,
            RemarkableApiError {status: response.status()}); 
    Ok(())    
}

fn do_upload_request(storage_url: &String, token: &str) -> Result<UploadDirections>{
    let payload = UploadRequest {
        ID: Uuid::new_v4().to_string(),
        Type: String::from("DocumentType"),
        Version: 1,
    };

    let mut url = storage_url.clone();
    url.push_str("/json/2/upload/request");

    let client = reqwest::blocking::Client::new();
    let response = client.put(&url)
        .bearer_auth(&token)    
        .body(serde_json::to_string(&payload).unwrap())
        .send().context(RemarkableApiUnreachable)?;

    ensure!(response.status() == reqwest::StatusCode::OK,
            RemarkableApiError {status: response.status()});      
    
    let text = response.text().unwrap();
    let upload_awnser: UploadDirections = serde_json::from_str(&text)
        .context(IncorrectApiResponse {response: text})?;

    Ok(upload_awnser)
}