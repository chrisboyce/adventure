use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::{Block, Borders, Paragraph, Widget},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::BufWriter, time::Duration};
use tokio::{sync::mpsc, time::sleep};
use tracing::info;

#[derive(Serialize)]
struct LLMRequest {
    model: String,
    stream: bool,
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

struct GameMap {
    width: usize,
    height: usize,
    tiles: Vec<Vec<Tile>>,
}
#[derive(Clone, Copy, Debug)]
enum Tile {
    Wall,
    Path,
    Item(Item),
    Character(Character),
    Unknown,
}
#[derive(Clone, Debug, Copy)]
enum Item {
    Key,
    Chest,
    Treasure,
}
#[derive(Clone, Debug, Copy)]
enum Character {
    Player,
    NPC,
    Enemy,
}
#[derive(Debug, Clone)]
struct NPC {
    x: isize,
    y: isize,
}

impl NPC {
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
impl GameMap {
    fn new(width: usize, height: usize) -> Self {
        let mut tiles = vec![vec![Tile::Path; width]; height];
        // Add some walls for testing
        // tiles[1][1] = Tile::Wall;
        tiles[1][1] = Tile::Item(Item::Treasure);
        tiles[3][3] = Tile::Character(Character::NPC);
        GameMap {
            width,
            height,
            tiles,
        }
    }

    fn get_tile(&self, x: isize, y: isize) -> Tile {
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
    let log_file = File::create("npc_debug.log")?;
    let log_writer = BufWriter::new(log_file);

    let file_appender = tracing_appender::rolling::daily("/tmp", "prefix.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
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
        terminal.draw(|frame| {
            let layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(frame.area());
            frame.render_widget(&GameMap::new(5, 5), layout[0]);
            frame.render_widget(
                Paragraph::new(current_action.clone()).block(Block::new().borders(Borders::ALL)),
                layout[1],
            );
            // let size = frame.size();
            // let chunks = Layout::default()
            //     .direction(Direction::Vertical)
            //     .margin(1)
            //     .constraints([Constraint::Min(1)].as_ref())
            //     .split(size);

            // let para = Paragraph::new(current_action.clone())
            //     .block(Block::default().borders(Borders::ALL).title("NPC Decision"))
            //     .style(Style::default());

            // frame.render_widget(para, chunks[0]);
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

    let map = GameMap::new(5, 5);
    let npc = NPC { x: 3, y: 3 };
    let view = npc.visible_tiles(&map);
    let prompt = generate_prompt_from_view(view);
    info!("Prompt: {prompt}");
    //     let prompt = r#"
    // You are an NPC in a tile-based game. Your goal is to explore. You see walls north and west, paths elsewhere.
    // Respond ONLY in this JSON format: { "action": "move", "target": "south" }
    // "#;

    let request = LLMRequest {
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
