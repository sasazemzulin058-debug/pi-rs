use anyhow::Result;
use pi_acp::{
    read_message, write_message, Error as AcpError, InitializeRequest, InitializeResponse,
};
use serde_json::{json, Value};
use std::io::{self, BufReader, BufWriter};

pub fn run() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    let mut initialized = false;
    loop {
        let message = match read_message(&mut reader) {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(()),
            Err(error @ AcpError::Json(_)) => {
                write_message(
                    &mut writer,
                    &json!({"jsonrpc":"2.0", "id":null, "error":{"code":-32700,"message":error.to_string()}}),
                )?;
                continue;
            }
            Err(
                error @ (AcpError::NotObject
                | AcpError::WrongVersion
                | AcpError::InvalidEnvelope(_)),
            ) => {
                write_message(
                    &mut writer,
                    &json!({"jsonrpc":"2.0", "id":null, "error":{"code":-32600,"message":error.to_string()}}),
                )?;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let Some(object) = message.as_object() else {
            continue;
        };
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = object.get("id") else {
            eprintln!("ACP notification rejected: {method}");
            continue;
        };
        if id.is_null() {
            write_message(
                &mut writer,
                &json!({"jsonrpc":"2.0", "id":null, "error":{"code":-32600,"message":"request id must not be null"}}),
            )?;
            continue;
        }
        let response = if method == "initialize" && !initialized {
            match object
                .get("params")
                .ok_or_else(|| "missing params".to_string())
                .and_then(|params| {
                    serde_json::from_value::<InitializeRequest>(params.clone())
                        .map_err(|e| e.to_string())
                }) {
                Ok(request) => {
                    initialized = true;
                    json!({"jsonrpc":"2.0", "id":id, "result": InitializeResponse::for_request(&request, env!("CARGO_PKG_VERSION"))})
                }
                Err(error) => {
                    json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32602,"message":error}})
                }
            }
        } else {
            json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601,"message":"method not supported"}})
        };
        write_message(&mut writer, &response)?;
    }
}
