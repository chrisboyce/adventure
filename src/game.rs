use ratatui::widgets::Widget;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::{
    LLMOptions, LLMResponse, Message,
    llm::{self, MODEL},
};

#[derive(Deserialize, Debug, Clone)]
pub struct NPCAction {
    pub action: String,
    pub target: Option<String>,
}

pub struct Context {
    pub previous_state: Option<GameState>,
    pub state: GameState,
}
impl Context {
    /// Move the current state into the previous state, and record the new
    /// state.
    pub fn update_state(&mut self, state: GameState) {
        self.previous_state = Some(std::mem::replace(&mut self.state, state));
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GameState {
    pub world: GameMap,
}
impl GameState {
    pub fn apply_action(&self, agent_id: &String, action: &NPCAction) -> Self {
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
                if let Some(agent) = new_agents.iter_mut().find(|agent| agent.id == *agent_id) {
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
pub struct GameMap {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Vec<Tile>>,
    pub agents: Vec<Agent>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Agent {
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
pub enum Tile {
    Wall,
    Path,
    Item(Item),
    Character(Character),
    Unknown,
    Empty,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy)]
pub enum Item {
    Key,
    Chest,
    Treasure,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy)]
pub enum Character {
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
                    Tile::Item(_item) => 'I',
                    Tile::Character(_character) => '╂',
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

pub async fn get_npc_action(
    agent_id: &String,
    state: Arc<RwLock<GameState>>,
) -> Result<NPCAction, Box<dyn std::error::Error>> {
    let client = Client::new();

    let state = state.read().await;
    let agent = state
        .world
        .agents
        .iter()
        .find(|agent| agent.id == *agent_id)
        .unwrap();
    let view = agent.visible_tiles(&state.world);
    let prompt = generate_prompt_from_view(view);
    info!("Prompt: {prompt}");

    let request = llm::LLMRequest {
        model: MODEL.to_string(),
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
