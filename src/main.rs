use crossterm::event::{self, Event, KeyCode};
use llm::{LLMOptions, LLMResponse, Message};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    cmp::{max, min},
    fs::File,
    io::BufWriter,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{RwLock, mpsc},
    time::{interval, sleep},
};
use tracing::info;
use tracing_subscriber::EnvFilter;

pub struct WorldMapWidget<'a> {
    game_state: &'a GameState,
    center_x: i32,
    center_y: i32,
}
impl<'a> Widget for WorldMapWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let GameState {
            world:
                GameMap {
                    width,
                    height,
                    tiles,
                    agents,
                },
        } = self.game_state;

        let map_height = *height as i32;
        let map_width = *width as i32;

        let view_width = area.width as i32;
        let view_height = area.height as i32;

        let min_x = max(0, self.center_x - view_width / 2);
        let min_y = max(0, self.center_y - view_height / 2);
        let max_x = min(map_width, min_x + view_width);
        let max_y = min(map_height, min_y + view_height);

        let mut agent_positions = vec![vec![None; map_width as usize]; map_height as usize];
        for agent in agents {
            if agent.y >= 0 && agent.y < map_height && agent.x >= 0 && agent.x < map_width {
                agent_positions[agent.y as usize][agent.x as usize] =
                    Some(agent.id.chars().next().unwrap_or('@'));
            }
        }

        for y in min_y..max_y {
            for x in min_x..max_x {
                let screen_x = (x - min_x) as u16 + area.x;
                let screen_y = (y - min_y) as u16 + area.y;

                let symbol = if let Some(ch) = agent_positions[y as usize][x as usize] {
                    ch.to_string()
                } else {
                    match tiles[y as usize][x as usize] {
                        Tile::Wall => "█".to_string(),
                        Tile::Empty => ".".to_string(),
                        Tile::Path => "~".to_string(),
                        Tile::Item(_) => "$".to_string(),
                        Tile::Character(character) => todo!(),
                        Tile::Unknown => todo!(),
                    }
                };

                let style = match tiles[y as usize][x as usize] {
                    Tile::Wall => Style::default().fg(Color::DarkGray),
                    Tile::Item(_) => Style::default().fg(Color::Yellow),
                    Tile::Empty => Style::default().fg(Color::White),
                    Tile::Path => Style::default().fg(Color::Gray),
                    Tile::Character(character) => todo!(),
                    Tile::Unknown => todo!(),
                };

                buf.set_string(screen_x, screen_y, symbol, style);
            }
        }
    }
}
mod llm {
    use derive_builder::Builder;
    use serde::{Deserialize, Serialize};
    const MODEL: &'static str = "gemma3";
    #[derive(Serialize)]
    pub(crate) struct LLMRequest {
        pub(crate) model: String,
        pub(crate) stream: bool,
        pub(crate) messages: Vec<Message>,
        pub(crate) options: LLMOptions,
    }
    #[derive(Serialize)]
    pub(crate) struct Message {
        pub(crate) role: String,
        pub(crate) content: String,
    }

    #[derive(Serialize)]
    pub(crate) struct LLMOptions {
        pub(crate) temperature: f32,
        pub(crate) response_format: String,
    }

    #[derive(Deserialize)]
    pub(crate) struct LLMResponse {
        pub(crate) message: LLMMessageContent,
    }

    #[derive(Deserialize)]
    pub(crate) struct LLMMessageContent {
        pub(crate) content: String,
    }
}

#[derive(Deserialize, Debug, Clone)]
struct NPCAction {
    action: String,
    target: Option<String>,
}

struct Context {
    previous_state: Option<GameState>,
    state: GameState,
}
impl Context {
    /// Move the current state into the previous state, and record the new
    /// state.
    pub fn update_state(&mut self, state: GameState) {
        self.previous_state = Some(std::mem::replace(&mut self.state, state));
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
struct GameState {
    world: GameMap,
}
impl GameState {
    fn apply_action(&self, action: &NPCAction) -> Self {
        if action.action == "move" {
            if let Some(dir) = action.target.as_deref() {
                // Compute delta from direction
                let (dx, dy) = match dir {
                    "north" => (0, -1),
                    "northeast" => (1, -1),
                    "east" => (1, 0),
                    "southeast" => (1, 1),
                    "south" => (0, 1),
                    "southwest" => (-1, 1),
                    "west" => (-1, 0),
                    "northwest" => (-1, -1),
                    _ => (0, 0),
                };
                info!(dx, dy, "Direction to dx/dy");

                let mut new_agents = self.world.agents.clone();

                // For now we move the first agent (or match by a fixed ID if preferred)
                if let Some(agent) = new_agents.first_mut() {
                    let new_x = (agent.x + dx).clamp(0, self.world.width as i32 - 1);
                    let new_y = (agent.y + dy).clamp(0, self.world.height as i32 - 1);
                    agent.x = new_x;
                    agent.y = new_y;
                }

                let new_world = GameMap {
                    width: self.world.width,
                    height: self.world.height,
                    tiles: self.world.tiles.clone(),
                    agents: new_agents,
                };

                return Self { world: new_world };
            }
        }

        // No valid action or unknown command, return unchanged
        self.clone()
    }
}
impl Default for GameState {
    fn default() -> Self {
        Self {
            world: GameMap::new(32, 16),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GameMap {
    width: usize,
    height: usize,
    tiles: Vec<Vec<Tile>>,
    agents: Vec<Agent>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Agent {
    pub id: String,
    pub x: i32,
    pub y: i32,
}
impl Agent {
    fn visible_tiles(&self, map: &GameMap) -> [[Tile; 3]; 3] {
        let mut view = [[Tile::Unknown; 3]; 3];
        for dy in -1..=1 {
            for dx in -1..=1 {
                view[(dy + 1) as usize][(dx + 1) as usize] = map.get_tile(self.x + dx, self.y + dy);
            }
        }
        view
    }
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
enum Tile {
    Wall,
    Path,
    Item(Item),
    Character(Character),
    Unknown,
    Empty,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy)]
enum Item {
    Key,
    Chest,
    Treasure,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy)]
enum Character {
    Player,
    NPC,
    Enemy,
}

impl GameMap {
    fn new(width: usize, height: usize) -> Self {
        let mut tiles = vec![vec![Tile::Path; width]; height];
        let agents = vec![Agent {
            id: "Alice".to_string(),
            x: 3,
            y: 3,
        }];
        tiles[2][2] = Tile::Item(Item::Treasure);
        GameMap {
            width,
            height,
            tiles,
            agents,
        }
    }

    fn get_tile(&self, x: i32, y: i32) -> Tile {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            self.tiles[y as usize][x as usize]
        } else {
            Tile::Wall
        }
    }
}
impl Widget for &GameMap {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        for x in area.left()..(area.right().min(self.width as u16)) {
            for y in area.top()..(area.bottom().min(self.height as u16)) {
                // let char = self.
                let tile_graphic = match &self.tiles[x as usize][y as usize] {
                    Tile::Wall => '▦',
                    Tile::Path => '░',
                    Tile::Item(item) => 'I',
                    Tile::Character(character) => '╂',
                    Tile::Unknown => '?',
                    Tile::Empty => todo!(),
                };
                buf.cell_mut((x, y))
                    .expect("Failed to fetch cell")
                    .set_char(tile_graphic);
            }
        }
    }
}
fn generate_prompt_from_view(view: [[Tile; 3]; 3]) -> String {
    let mut description = String::from(
        "You are an NPC in a tile-based game. \
        You are seeking treasure. If you see \
        treasure, you should move towards it. \
        Here's what you see:\n",
    );

    for (dy, row) in view.iter().enumerate() {
        for (dx, tile) in row.iter().enumerate() {
            let direction = match (dy as isize - 1, dx as isize - 1) {
                (-1, 0) => "north",
                (1, 0) => "south",
                (0, -1) => "west",
                (0, 1) => "east",
                (-1, -1) => "northwest",
                (-1, 1) => "northeast",
                (1, -1) => "southwest",
                (1, 1) => "southeast",
                (0, 0) => "your current position",
                _ => "somewhere",
            };

            let desc = match tile {
                Tile::Wall => "a wall",
                Tile::Path => "a path",
                Tile::Item(c) => &format!("An item: {:?}", c),
                Tile::Character(c) => &format!("Another character '{:?}'", c),
                Tile::Unknown => "unknown terrain",
                Tile::Empty => todo!(),
            };

            description.push_str(&format!("- To the {}: {}\n", direction, desc));
        }
    }

    description.push_str("Respond in JSON: { \"action\": \"move\", \"target\": DIRECTION }");
    info!("View: \n{:?}", description);
    description
}

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

    let (tx, mut rx) = mpsc::channel::<NPCAction>(1);
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

                if tx.send(action).await.is_err() {
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
                // frame.render_widget(&GameMap::new(5, 5), layout[0]);
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
        if let Ok(action) = rx.try_recv() {
            let state = state.read().await;
            let new_state = state.apply_action(&action);
            game_state_tx.send(new_state).await.unwrap();

            current_action = format!(
                "Action: {}, Target: {}",
                action.action,
                action.target.unwrap_or("None".to_string())
            );
            // tx.send(action).await.ok();
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

async fn get_npc_action(
    agent_id: &String,
    state: Arc<RwLock<GameState>>,
) -> Result<NPCAction, Box<dyn std::error::Error>> {
    let client = Client::new();

    // let map = GameMap::new(5, 5);
    let state = state.read().await;
    let agent = state
        .world
        .agents
        .iter()
        .find(|agent| agent.id == *agent_id)
        .unwrap();
    let view = agent.visible_tiles(&state.world);
    // let npc = NPC { x: 3, y: 3 };
    // let view = npc.visible_tiles(&state.world);
    let prompt = generate_prompt_from_view(view);
    info!("Prompt: {prompt}");
    //     let prompt = r#"
    // You are an NPC in a tile-based game. Your goal is to explore. You see walls north and west, paths elsewhere.
    // Respond ONLY in this JSON format: { "action": "move", "target": "south" }
    // "#;

    let request = llm::LLMRequest {
        model: "gemma3".to_string(),
        stream: false,
        messages: vec![Message {
            role: "user".into(),
            content: prompt.into(),
        }],
        options: LLMOptions {
            temperature: 0.7,
            response_format: "json".to_string(),
        },
    };

    let res = client
        .post("http://localhost:11434/api/chat")
        .json(&request)
        .send()
        .await?;

    let text = res.text().await?;
    info!("Raw LLM response text: [{}]", text);

    let response: LLMResponse = serde_json::from_str(&text)?;
    info!("Contents [{}]", &response.message.content);
    let content = &response.message.content;
    let opening = content.find("{").unwrap();
    let closing = content.rfind("}").unwrap();
    let json_string = &content[opening..=closing];

    let action: NPCAction = serde_json::from_str(json_string)?;
    Ok(action)
}
fn render(frame: &mut Frame<'_>) {}
