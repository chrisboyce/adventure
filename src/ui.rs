use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use std::cmp::{max, min};

use crate::{
    GameState,
    game::{GameMap, Tile},
};

pub struct WorldMapWidget<'a> {
    pub game_state: &'a GameState,
    pub center_x: i32,
    pub center_y: i32,
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
                        Tile::Character(_character) => todo!(),
                        Tile::Unknown => todo!(),
                    }
                };

                let style = match tiles[y as usize][x as usize] {
                    Tile::Wall => Style::default().fg(Color::DarkGray),
                    Tile::Item(_) => Style::default().fg(Color::Yellow),
                    Tile::Empty => Style::default().fg(Color::White),
                    Tile::Path => Style::default().fg(Color::Gray),
                    Tile::Character(_character) => todo!(),
                    Tile::Unknown => todo!(),
                };

                buf.set_string(screen_x, screen_y, symbol, style);
            }
        }
    }
}
