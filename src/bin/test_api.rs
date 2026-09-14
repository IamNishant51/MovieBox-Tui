use moviebox_tui::providers::moviebox::client::MovieBoxClient;

#[tokio::main]
async fn main() {
    let client = MovieBoxClient::new();
    client.init().await.unwrap();
    
    let res = client.search("hello", 1).await.unwrap();
    println!("{}", serde_json::to_string_pretty(&res).unwrap());
}
