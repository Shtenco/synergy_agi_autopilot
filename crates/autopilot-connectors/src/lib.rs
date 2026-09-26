use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};

use autopilot_secrets::{SecretRef, SecretVault};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("unsupported URL: {0}")]
    UnsupportedUrl(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("JSON error: {0}")]
    Json(String),
    #[error("OAuth token unavailable")]
    OAuthUnavailable,
    #[error("database operation denied: {0}")]
    DbDenied(String),
    #[error("database error: {0}")]
    Db(String),
    #[error("queue lock poisoned")]
    QueuePoisoned,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OAuth2Config {
    pub token_url: String,
    pub client_id_secret: String,
    pub client_secret_secret: String,
    pub scope: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct OAuth2Token {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct OAuth2Client<V: SecretVault + Clone> {
    config: OAuth2Config,
    vault: V,
    cached: Arc<Mutex<Option<OAuth2Token>>>,
}

impl<V: SecretVault + Clone> OAuth2Client<V> {
    pub fn new(config: OAuth2Config, vault: V) -> Self {
        Self { config, vault, cached: Arc::new(Mutex::new(None)) }
    }

    pub fn token(&self) -> Result<OAuth2Token, ConnectorError> {
        if let Ok(cache) = self.cached.lock() {
            if let Some(token) = cache.as_ref() {
                if token.expires_at > Utc::now() + Duration::seconds(30) {
                    return Ok(token.clone());
                }
            }
        }
        let client_id = self.vault.resolve(&SecretRef::new(self.config.client_id_secret.clone()).map_err(|e| ConnectorError::Http(e.to_string()))?)
            .map_err(|e| ConnectorError::Http(e.to_string()))?;
        let client_secret = self.vault.resolve(&SecretRef::new(self.config.client_secret_secret.clone()).map_err(|e| ConnectorError::Http(e.to_string()))?)
            .map_err(|e| ConnectorError::Http(e.to_string()))?;
        let form = format!(
            "grant_type=client_credentials&client_id={}&client_secret={}&scope={}",
            percent_encode(client_id.expose()),
            percent_encode(client_secret.expose()),
            percent_encode(&self.config.scope.join(" "))
        );
        let value = http_request_json(&self.config.token_url, "POST", &[
            ("Content-Type".into(),"application/x-www-form-urlencoded".into())
        ], Some(form.as_bytes()))?;
        let access_token = value.get("access_token").and_then(Value::as_str)
            .ok_or(ConnectorError::OAuthUnavailable)?.to_string();
        let expires_in = value.get("expires_in").and_then(Value::as_i64).unwrap_or(3600).max(60);
        let token = OAuth2Token { access_token, expires_at: Utc::now() + Duration::seconds(expires_in) };
        if let Ok(mut cache) = self.cached.lock() { *cache = Some(token.clone()); }
        Ok(token)
    }
}

fn percent_encode(value: &str) -> String {
    value.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{:02X}", b),
    }).collect::<Vec<_>>().join("")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenApiOperation {
    pub operation_id: String,
    pub method: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenApiSpec {
    pub base_url: String,
    pub operations: BTreeMap<String, OpenApiOperation>,
}

impl OpenApiSpec {
    pub fn from_json(base_url: impl Into<String>, document: &Value) -> Self {
        let mut operations = BTreeMap::new();
        if let Some(paths) = document.get("paths").and_then(Value::as_object) {
            for (path, item) in paths {
                let Some(methods) = item.as_object() else { continue };
                for (method, op) in methods {
                    if !matches!(method.as_str(), "get"|"post"|"put"|"patch"|"delete") { continue; }
                    let Some(operation_id) = op.get("operationId").and_then(Value::as_str) else { continue };
                    operations.insert(operation_id.to_string(), OpenApiOperation {
                        operation_id: operation_id.to_string(),
                        method: method.to_ascii_uppercase(),
                        path: path.clone(),
                    });
                }
            }
        }
        Self { base_url: base_url.into(), operations }
    }
}

pub struct OpenApiClient<V: SecretVault + Clone> {
    pub spec: OpenApiSpec,
    pub oauth: Option<OAuth2Client<V>>,
}

impl<V: SecretVault + Clone> OpenApiClient<V> {
    pub fn call(&self, operation_id: &str, path_params: &BTreeMap<String,String>, body: Option<&Value>) -> Result<Value, ConnectorError> {
        let op = self.spec.operations.get(operation_id)
            .ok_or_else(|| ConnectorError::Http(format!("unknown operationId: {operation_id}")))?;
        let mut path = op.path.clone();
        for (key, value) in path_params {
            path = path.replace(&format!("{{{key}}}"), value);
        }
        if path.contains('{') { return Err(ConnectorError::Http(format!("unresolved path params: {path}"))); }
        let url = format!("{}{}", self.spec.base_url.trim_end_matches('/'), path);
        let mut headers = vec![("Accept".into(),"application/json".into())];
        if let Some(oauth) = &self.oauth {
            headers.push(("Authorization".into(), format!("Bearer {}", oauth.token()?.access_token)));
        }
        let payload = body.map(|v| serde_json::to_vec(v).map_err(|e| ConnectorError::Json(e.to_string()))).transpose()?;
        if payload.is_some() { headers.push(("Content-Type".into(),"application/json".into())); }
        http_request_json(&url, &op.method, &headers, payload.as_deref())
    }
}

pub fn http_request_json(url: &str, method: &str, headers: &[(String,String)], body: Option<&[u8]>) -> Result<Value, ConnectorError> {
    let url = Url::parse(url).map_err(|_| ConnectorError::UnsupportedUrl(url.into()))?;
    if url.scheme() != "http" { return Err(ConnectorError::UnsupportedUrl("only http:// is supported by the minimal native connector".into())); }
    let host = url.host_str().ok_or_else(|| ConnectorError::UnsupportedUrl(url.to_string()))?;
    let port = url.port_or_known_default().ok_or_else(|| ConnectorError::UnsupportedUrl(url.to_string()))?;
    let mut target = url.path().to_string();
    if target.is_empty() { target = "/".into(); }
    if let Some(query) = url.query() { target.push('?'); target.push_str(query); }
    let mut request = format!("{method} {target} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n");
    for (key,value) in headers { request.push_str(&format!("{key}: {value}\r\n")); }
    if let Some(body) = body { request.push_str(&format!("Content-Length: {}\r\n", body.len())); }
    request.push_str("\r\n");
    let mut stream = TcpStream::connect((host,port)).map_err(|e| ConnectorError::Http(e.to_string()))?;
    stream.write_all(request.as_bytes()).map_err(|e| ConnectorError::Http(e.to_string()))?;
    if let Some(body)=body { stream.write_all(body).map_err(|e| ConnectorError::Http(e.to_string()))?; }
    let mut raw=Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| ConnectorError::Http(e.to_string()))?;
    let text=String::from_utf8_lossy(&raw);
    let (head,body)=text.split_once("\r\n\r\n").ok_or_else(|| ConnectorError::Http("invalid HTTP response".into()))?;
    let status=head.lines().next().and_then(|l|l.split_whitespace().nth(1)).and_then(|s|s.parse::<u16>().ok()).unwrap_or(0);
    let value=if body.trim().is_empty(){json!({})}else{serde_json::from_str(body).map_err(|e|ConnectorError::Json(e.to_string()))?};
    if !(200..300).contains(&status){return Err(ConnectorError::Http(format!("status={status} body={value}")));}
    Ok(value)
}

pub struct ReadOnlySqlite {
    conn: Connection,
}

impl ReadOnlySqlite {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ConnectorError> {
        Connection::open(path).map(|conn|Self{conn}).map_err(|e|ConnectorError::Db(e.to_string()))
    }
    pub fn query_json(&self, sql:&str)->Result<Vec<BTreeMap<String,Value>>,ConnectorError>{
        let trimmed=sql.trim_start().to_ascii_lowercase();
        if !(trimmed.starts_with("select ")||trimmed.starts_with("with ")){
            return Err(ConnectorError::DbDenied("only SELECT/CTE queries are allowed".into()));
        }
        if trimmed.contains(';'){return Err(ConnectorError::DbDenied("multiple statements are not allowed".into()));}
        let mut stmt=self.conn.prepare(sql).map_err(|e|ConnectorError::Db(e.to_string()))?;
        let names=stmt.column_names().into_iter().map(str::to_string).collect::<Vec<_>>();
        let rows=stmt.query_map([],|row|{
            let mut map=BTreeMap::new();
            for (i,name) in names.iter().enumerate(){
                let v:rusqlite::types::Value=row.get(i)?;
                map.insert(name.clone(),sqlite_value(v));
            }
            Ok(map)
        }).map_err(|e|ConnectorError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(|e|ConnectorError::Db(e.to_string()))
    }
}
fn sqlite_value(v:rusqlite::types::Value)->Value{
    match v{
        rusqlite::types::Value::Null=>Value::Null,
        rusqlite::types::Value::Integer(v)=>Value::from(v),
        rusqlite::types::Value::Real(v)=>Value::from(v),
        rusqlite::types::Value::Text(v)=>Value::from(v),
        rusqlite::types::Value::Blob(v)=>Value::from(v.iter().map(|b|format!("{b:02x}")).collect::<String>()),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueMessage { pub id:String, pub body:Value }

#[derive(Clone,Default)]
pub struct InMemoryQueue { inner:Arc<Mutex<VecDeque<QueueMessage>>> }
impl InMemoryQueue{
    pub fn publish(&self,msg:QueueMessage)->Result<(),ConnectorError>{self.inner.lock().map_err(|_|ConnectorError::QueuePoisoned)?.push_back(msg);Ok(())}
    pub fn consume(&self)->Result<Option<QueueMessage>,ConnectorError>{Ok(self.inner.lock().map_err(|_|ConnectorError::QueuePoisoned)?.pop_front())}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn parses_openapi_operations(){
        let doc=json!({"paths":{"/leads/{id}":{"get":{"operationId":"getLead"}}}});
        let spec=OpenApiSpec::from_json("http://127.0.0.1:1",&doc);
        assert_eq!(spec.operations["getLead"].method,"GET");
    }
    #[test] fn readonly_db_denies_writes(){
        let db=ReadOnlySqlite{conn:Connection::open_in_memory().unwrap()};
        assert!(matches!(db.query_json("DELETE FROM x"),Err(ConnectorError::DbDenied(_))));
    }
    #[test] fn queue_roundtrip(){
        let q=InMemoryQueue::default();q.publish(QueueMessage{id:"1".into(),body:json!({"x":1})}).unwrap();
        assert_eq!(q.consume().unwrap().unwrap().body["x"],1);
    }
}
