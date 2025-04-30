use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::{Block, Borders, Paragraph},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{io, time::Duration};
use tokio::{sync::mpsc, time::sleep};

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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::channel::<NPCAction>(1);

    // Start async LLM query
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

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
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
