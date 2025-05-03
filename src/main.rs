use crossterm::event::{self, Event, KeyCode};
use game::{GameState, NPCAction, get_npc_action};
use llm::{LLMOptions, LLMResponse, Message};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{RwLock, mpsc},
    time::{interval, sleep},
};
use tracing::info;
use ui::WorldMapWidget;
mod game;
mod llm;
mod ui;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_appender = tracing_appender::rolling::daily("/tmp", "llm_log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        // .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Application started");
    let mut terminal = ratatui::init();

    let (tx, mut rx) = mpsc::channel::<(String, NPCAction)>(1);
    let (game_state_tx, mut game_state_rx) = mpsc::channel::<GameState>(1);
    let state = Arc::new(RwLock::new(GameState::default()));
    let state_b = state.clone();

    let alice = String::from("Alice");
    tokio::spawn({
        let state = Arc::clone(&state); // Clone the Arc so we can move it into the task
        async move {
            let mut ticker = interval(Duration::from_secs(5));

            loop {
                ticker.tick().await;

                let action =
                    get_npc_action(&alice, Arc::clone(&state))
                        .await
                        .unwrap_or(NPCAction {
                            action: "error".into(),
                            target: None,
                        });

                if tx.send((alice.clone(), action)).await.is_err() {
                    // Receiver dropped; exit the loop
                    break;
                }
            }
        }
    });
    tokio::spawn(async move {
        loop {
            let msg = game_state_rx.recv().await;

            if let Some(msg) = msg {
                // info!(?msg, "Received new game state");
                let mut state = state_b.write().await;
                *state = msg;
            }
        }
    });

    // Default display value
    let mut current_action = "Waiting for NPC...".to_string();

    loop {
        {
            let state = state.read().await;
            terminal.draw(|frame| {
                let layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(frame.area());

                frame.render_widget(
                    WorldMapWidget {
                        game_state: &state,
                        center_x: 2,
                        center_y: 2,
                    },
                    layout[0],
                );
                frame.render_widget(
                    Paragraph::new(current_action.clone())
                        .block(Block::new().borders(Borders::ALL)),
                    layout[1],
                );
            })?;
        }

        // Update action if a message arrives
        if let Ok((agent_id, action)) = rx.try_recv() {
            let state = state.read().await;
            let new_state = state.apply_action(&agent_id, &action);
            game_state_tx.send(new_state).await.unwrap();

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
