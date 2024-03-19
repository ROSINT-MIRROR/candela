use clap::{Arg, ArgMatches, Command};
use fantoccini::{ClientBuilder, Locator};
use futures::stream::{self, StreamExt};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONNECTION, REFERER, USER_AGENT};
use reqwest::Client;
use reqwest::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio;
use tokio::fs;
use tokio::io::{self, AsyncWriteExt};

#[derive(Serialize, Deserialize, Debug)]
struct SherlockSite {
    error_code: Option<String>,
    error_msg: Option<String>,
    #[serde(default)] // If the field may be omitted or is not present at all times.
    #[serde(rename = "errorType")]
    error_type: Option<String>, //
    error_url: Option<String>,
    headers: Option<HashMap<String, String>>,
    #[serde(rename = "isNSFW")]
    is_nsfw: Option<bool>,
    #[serde(rename = "noPeriod")]
    no_period: Option<String>,
    #[serde(rename = "regexCheck")]
    regex_check: Option<String>,
    #[serde(rename = "request_method")]
    request_method: Option<String>,
    #[serde(rename = "request_payload")]
    request_payload: Option<String>,
    url: String,
    url_main: Option<String>,
    #[serde(rename = "urlProbe")]
    url_probe: Option<String>,
    #[serde(rename = "username_claimed")]
    username_claimed: Option<String>,
    #[serde(rename = "username_unclaimed")]
    username_unclaimed: Option<String>,
}

#[tokio::main]
async fn main() {
    let matches = Command::new("candela")
        .version("0.1")
        .author("TragDate")
        .about("Shines light on the availability of a username on various social media platforms")
        .arg(Arg::new("USERNAME").help("Sets the username to check").required(true).index(1))
        .arg(Arg::new("cgi").long("cgi").takes_value(false).help("Runs in CGI mode"))
        .get_matches();
    if matches.is_present("cgi") {
        run_cgi(matches).await;
    } else {
        run_cli(matches).await;
    }
}

async fn run_cli(matches: ArgMatches) {
    let username = matches.value_of("USERNAME").unwrap();
    let mut sites: HashMap<String, SherlockSite> = get_sites().await.unwrap();
    sites.retain(|_, site| site.error_type.as_deref() == Some("status_code"));
    let client = build_client();
    let results: Vec<_> = sites
        .into_iter()
        .map(|(name, site)| {
            let client = client.clone();
            let replaced_url = site.url.replace("{}", username);
            tokio::spawn(async move {
                if let Ok(true) = status_check(&client, &replaced_url).await {
                    println!("{}: {}", name, replaced_url);
                }
            })
        })
        .collect();
    let _results: Vec<_> = stream::iter(results).buffer_unordered(10).collect().await;
}

async fn run_cgi(matches: ArgMatches) {
    let username = matches.value_of("USERNAME").unwrap();
    let mut sites: HashMap<String, SherlockSite> = get_sites().await.unwrap();
    sites.retain(|_, site| site.error_type.as_deref() == Some("status_code"));
    println!("Content-Type: text/event-stream");
    println!("Cache-Control: no-cache");
    println!("Connection: keep-alive");
    println!();
    io::stdout().flush().await.unwrap();
    let client = build_client();
    let _results: Vec<_> = stream::iter(sites.into_iter().map(|(_name, site)| {
        let client = client.clone();
        let replaced_url = site.url.replace("{}", username);
        tokio::spawn(async move {
            if let Ok(true) = status_check(&client, &replaced_url).await {
                let sse_data = format!("data: {}\n\n", replaced_url);
                io::stdout().write_all(sse_data.as_bytes()).await.expect("Failed to write to stdout");
                io::stdout().flush().await.expect("Failed to flush stdout");
            }
        })
    }))
    .buffer_unordered(10)
    .collect()
    .await;

    // Now, after all the data has been sent, close the connection
    println!("data: {}\n\n", "done");
    io::stdout().flush().await.expect("Failed to flush stdout");
    std::process::exit(0);
}

async fn get_sites() -> Result<HashMap<String, SherlockSite>, Error> {
    let data = fs::read_to_string("data.json").await.unwrap();
    let sites: HashMap<String, SherlockSite> = serde_json::from_str(&data).unwrap();
    Ok(sites)
}

fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(3))
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                USER_AGENT,
                "Mozilla/5.0 (Windows NT 6.1) AppleWebKit/537.36 (KHTML, like Gecko) Brave Chrome/85.0.4183.121 Safari/537.36"
                    .parse()
                    .unwrap(),
            );
            headers.insert(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().unwrap());
            headers.insert(ACCEPT_LANGUAGE, "en-US,en;q=0.5".parse().unwrap());
            headers.insert(ACCEPT_ENCODING, "gzip, deflate, br".parse().unwrap());
            headers.insert(CONNECTION, "keep-alive".parse().unwrap());
            headers.insert(REFERER, "https://www.google.com/".parse().unwrap());
            headers
        })
        .build()
        .expect("Client should be built")
}

async fn status_check(client: &Client, url: &str) -> Result<bool, reqwest::Error> {
    let res = client.get(url).send().await?;
    Ok(res.status().is_success())
}

async fn bloated_check(url: String, wait: String, check_selector: String) -> Result<bool, fantoccini::error::CmdError> {
    let client = ClientBuilder::native().connect("http://localhost:4444").await.unwrap();
    client.goto(&url).await?;
    client.wait().for_element(Locator::Css(&wait)).await?;

    let result = match client.find(Locator::Css(&check_selector)).await {
        Ok(_) => true,
        Err(fantoccini::error::CmdError::NoSuchElement(_)) => false,
        Err(e) => return Err(e),
    };
    let _ = client.close().await;
    Ok(result)
}
