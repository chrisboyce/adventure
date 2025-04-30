use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::{Block, Borders, Paragraph},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::BufWriter, time::Duration};
use tokio::{sync::mpsc, time::sleep};
use tracing::info;

#[derive(Serialize)]
struct LLMRequest {
    model: String,
    messages: Vec<Message>,
    options: LLMOptions,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct LLMOptions {
    temperature: f32,
    response_format: String,
}

#[derive(Deserialize)]
struct LLMResponse {
    message: LLMMessageContent,
}

#[derive(Deserialize)]
struct LLMMessageContent {
    content: String,
}

#[derive(Deserialize, Debug, Clone)]
struct NPCAction {
    action: String,
    target: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_file = File::create("npc_debug.log")?;
    let log_writer = BufWriter::new(log_file);

    tracing_subscriber::fmt()
        // .with_writer(std::fs::File::create("debug.log"))
        // .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Application started");
    let mut terminal = ratatui::init();

    let (tx, mut rx) = mpsc::channel::<NPCAction>(1);

    tokio::spawn(async move {
        let action = get_npc_action().await.unwrap_or(NPCAction {
            action: "error".into(),
            target: None,
        });
        tx.send(action).await.ok();
    });

    // Default display value
    let mut current_action = "Waiting for NPC...".to_string();

    loop {
        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Min(1)].as_ref())
                .split(size);

            let para = Paragraph::new(current_action.clone())
                .block(Block::default().borders(Borders::ALL).title("NPC Decision"))
                .style(Style::default());

            f.render_widget(para, chunks[0]);
        })?;

        // Update action if a message arrives
        if let Ok(action) = rx.try_recv() {
            current_action = format!(
                "Action: {}, Target: {}",
                action.action,
                action.target.unwrap_or("None".to_string())
            );
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        sleep(Duration::from_millis(16)).await;
    }

    ratatui::restore();
    Ok(())
}

async fn get_npc_action() -> Result<NPCAction, Box<dyn std::error::Error>> {
    let client = Client::new();

    let prompt = r#"
You are an NPC in a tile-based game. Your goal is to explore. You see walls north and west, paths elsewhere.
Respond ONLY in this JSON format: { "action": "move", "target": "south" }
"#;

    let request = LLMRequest {
        model: "gemma3".to_string(),
        messages: vec![Message {
            role: "user".into(),
            content: prompt.into(),
        }],
        options: LLMOptions {
            temperature: 0.7,
            response_format: "json".to_string(),
        },
    };

    let response: LLMResponse = client
        .post("http://localhost:11434/api/chat")
        .json(&request)
        .send()
        .await?
        .json()
        .await?;

    let action: NPCAction = serde_json::from_str(&response.message.content)?;
    Ok(action)
}
