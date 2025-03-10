use std::env;

use anyhow::{bail, Context, Result};
use serde::de::{DeserializeOwned, IgnoredAny};
use tokio::io::BufReader;
use tokio::net::TcpStream;

use crate::config::Config;
use crate::lsp::ext::{self, LspMuxOptions, StatusResponse};
use crate::lsp::jsonrpc::{Message, Request, RequestId, Version};
use crate::lsp::transport::{LspReader, LspWriter};
use crate::lsp::{InitializationOptions, InitializeParams};

pub async fn ext_request<T>(method: ext::Request) -> Result<T>
where
    T: DeserializeOwned,
{
    let config = Config::load_or_default().await;

    let (reader, writer) = TcpStream::connect(config.connect)
        .await
        .context("connect")?
        .into_split();
    let mut writer = LspWriter::new(writer, "lspmux");
    let mut reader = LspReader::new(BufReader::new(reader), "lspmux");

    writer
        .write_message(
            &Request {
                jsonrpc: Version,
                method: "initialize".into(),
                params: serde_json::to_value(InitializeParams {
                    initialization_options: Some(InitializationOptions {
                        lsp_mux: Some(LspMuxOptions {
                            version: LspMuxOptions::PROTOCOL_VERSION.into(),
                            method,
                        }),
                        other_options: serde_json::Map::default(),
                    }),
                    process_id: None,
                    client_info: None,
                    locale: None,
                    root_path: None,
                    root_uri: None,
                    capabilities: None,
                    trace: None,
                    workspace_folders: Vec::new(),
                })
                .unwrap(),
                id: RequestId::Number(0),
            }
            .into(),
        )
        .await
        .context("send lspmux request")?;

    match reader
        .read_message()
        .await
        .context("read lspmux response")?
        .context("stream ended")?
        .into_response()
        .context("received message was not a response")?
    {
        Ok(success) => serde_json::from_value(success.result).context("parse response result"),
        Err(error) => bail!(
            "received error response: {msg:?}",
            msg = Message::ResponseError(error),
        ),
    }
}

pub async fn status(json: bool) -> Result<()> {
    let res = ext_request::<StatusResponse>(ext::Request::Status {}).await?;

    if json {
        let json = serde_json::to_string(&res).unwrap();
        println!("{json}");
        return Ok(());
    }

    for instance in res.instances {
        println!("- Instance pid {}", instance.pid);
        println!("  server: {:?} {:?}", instance.server, instance.args);
        println!("  path: {:?}", instance.workspace_root);
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        println!("  last used: {}s ago", now - instance.last_used);
        println!("  clients:");
        for client in instance.clients {
            println!("  - Client port {}", client.port);
        }
    }
    Ok(())
}

pub async fn reload() -> Result<()> {
    let cwd = env::current_dir()
        .context("unable to get current_dir")?
        .to_str()
        .context("current_dir is not valid utf-8")?
        .to_owned();
    ext_request::<IgnoredAny>(ext::Request::Reload { cwd }).await?;
    Ok(())
}
