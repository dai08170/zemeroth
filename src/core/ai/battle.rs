use core::command::{self, Command};
use core::map::{self, HexMap};
use core::movement::{self, Path, Pathfinder};
use core::state;
use core::utils::shuffle_vec;
use core::{self, check, ObjId, PlayerId, State};

// TODO: rename
enum XxxPath {
    Path(Path),
    CantFindPath,
    DontNeedToMove,
}

#[derive(Debug, Clone)]
pub struct Ai {
    id: PlayerId,

    distance_map: HexMap<bool>,

    /// Each AI has its own Pathfinder because it's not a part of the game state.
    pathfinder: Pathfinder,
}

impl Ai {
    pub fn new(id: PlayerId, map_radius: map::Distance) -> Self {
        Self {
            id,
            pathfinder: Pathfinder::new(map_radius),
            distance_map: HexMap::new(map_radius),
        }
    }

    /// Finds shortest path to some enemy.
    fn find_path_to_nearest_enemy(&mut self, state: &State, unit_id: ObjId) -> Option<Path> {
        self.pathfinder.fill_map(state, unit_id);
        let mut best_path = None;
        let mut best_cost = movement::max_cost();
        for &target_id in &shuffle_vec(state::enemy_agent_ids(state, self.id)) {
            let target_pos = state.parts().pos.get(target_id).0;
            for dir in map::dirs() {
                let pos = map::Dir::get_neighbor_pos(target_pos, dir);
                if !state.map().is_inboard(pos) {
                    continue;
                }
                let path = match self.pathfinder.path(pos) {
                    Some(path) => path,
                    None => continue,
                };
                let cost = path.cost_for(state, unit_id);
                if best_cost > cost {
                    best_cost = cost;
                    best_path = Some(path);
                }
            }
        }
        best_path
    }

    fn find_path_to_preserve_distance(&mut self, state: &State, unit_id: ObjId) -> Option<Path> {
        // clean the map
        for pos in self.distance_map.iter() {
            self.distance_map.set_tile(pos, false);
        }

        let distance_min = map::Distance(2);
        let distance_max = map::Distance(4);
        for pos in self.distance_map.iter() {
            for &enemy_id in &state::enemy_agent_ids(state, self.id) {
                let enemy_pos = state.parts().pos.get(enemy_id).0;
                if map::distance_hex(pos, enemy_pos) <= distance_max {
                    self.distance_map.set_tile(pos, true);
                }
            }
            for &enemy_id in &state::enemy_agent_ids(state, self.id) {
                let enemy_pos = state.parts().pos.get(enemy_id).0;
                if map::distance_hex(pos, enemy_pos) <= distance_min {
                    self.distance_map.set_tile(pos, false);
                }
            }
        }

        self.pathfinder.fill_map(state, unit_id);
        // TODO: remove code duplication
        let mut best_path = None;
        let mut best_cost = movement::max_cost();
        for pos in self.distance_map.iter() {
            if !self.distance_map.tile(pos) {
                continue;
            }
            let path = match self.pathfinder.path(pos) {
                Some(path) => path,
                None => continue,
            };
            let cost = path.cost_for(state, unit_id);
            if best_cost > cost {
                best_cost = cost;
                best_path = Some(path);
            }
        }
        best_path
    }

    fn find_any_path(&mut self, state: &State, unit_id: ObjId) -> Option<Path> {
        // let distance_min = map::Distance(2);
        // let distance_min = map::Distance(1);
        self.pathfinder.fill_map(state, unit_id);
        let mut best_path = None;
        let mut best_distance = map::Distance(11); // TODO: radius * 2 + 1?
        for pos in self.distance_map.iter() {
            let path = match self.pathfinder.path(pos) {
                Some(path) => path,
                None => continue,
            };
            for &enemy_id in &state::enemy_agent_ids(state, self.id) {
                let enemy_pos = state.parts().pos.get(enemy_id).0;
                let distance = map::distance_hex(pos, enemy_pos);
                // TODO: compare path costs
                // if distance <= best_distance && distance >= distance_min {
                if distance <= best_distance {
                    best_path = Some(path.clone());
                    best_distance = distance;
                }
            }
        }
        best_path
    }

    fn try_throw_bomb(&self, state: &State, unit_id: ObjId) -> Option<Command> {
        // TODO: find ability in the parts and use it here:
        let ability = core::ability::Ability::Bomb(core::ability::Bomb(map::Distance(3)));
        for &target_id in &shuffle_vec(state::enemy_agent_ids(state, self.id)) {
            let target_pos = state.parts().pos.get(target_id).0;
            for dir in shuffle_vec(map::dirs().collect()) {
                let pos = map::Dir::get_neighbor_pos(target_pos, dir);
                if !state.map().is_inboard(pos) || state::is_tile_blocked(state, pos) {
                    continue;
                }
                let command = Command::UseAbility(command::UseAbility {
                    id: unit_id,
                    pos,
                    ability,
                });
                if check(state, &command).is_ok() {
                    return Some(command);
                }
            }
        }
        None
    }

    fn try_summon_imp(&self, state: &State, unit_id: ObjId) -> Option<Command> {
        // TODO: find ability in the parts and use it here:
        let ability = core::ability::Ability::Summon(core::ability::Summon(3));
        // let ability = core::ability::Ability::Summon(core::ability::Summon(5));
        let target_pos = state.parts().pos.get(unit_id).0;
        let command = Command::UseAbility(command::UseAbility {
            id: unit_id,
            pos: target_pos,
            ability,
        });
        if check(state, &command).is_ok() {
            return Some(command);
        }
        None
    }

    fn try_to_attack(&self, state: &State, unit_id: ObjId) -> Option<Command> {
        for &target_id in &shuffle_vec(state::enemy_agent_ids(state, self.id)) {
            let command = Command::Attack(command::Attack {
                attacker_id: unit_id,
                target_id,
            });
            if check(state, &command).is_ok() {
                return Some(command);
            }
        }
        None
    }

    fn try_to_move_closer(&mut self, state: &State, unit_id: ObjId) -> XxxPath {
        let path = match self.find_path_to_nearest_enemy(state, unit_id) {
            Some(path) => path,
            None => return XxxPath::CantFindPath,
        };
        println!("try_to_move_closer: path = {:?}", path);
        if path.tiles().len() == 1 {
            println!("try_to_move_closer: IDEAL POSITION");
            return XxxPath::DontNeedToMove;
        }
        let path = match path.truncate(state, unit_id) {
            Some(path) => path,
            None => return XxxPath::CantFindPath,
        };
        let cost = path.cost_for(state, unit_id);
        let agent = state.parts().agent.get(unit_id);
        if agent.move_points < cost {
            return XxxPath::CantFindPath;
        }
        let command = Command::MoveTo(command::MoveTo { id: unit_id, path: path.clone() });
        if check(state, &command).is_ok() {
            return XxxPath::Path(path);
        }
        XxxPath::CantFindPath
    }

    fn try_to_keep_distance(&mut self, state: &State, unit_id: ObjId) -> XxxPath {
        let path = match self.find_path_to_preserve_distance(state, unit_id) {
            Some(path) => path,
            None => {
                println!("try_to_keep_distance: find_path_to_preserve_distance: None");
                return XxxPath::CantFindPath;
            }
        };
        println!("try_to_keep_distance: path = {:?}", path);
        if path.tiles().len() == 1 {
            println!("try_to_keep_distance: IDEAL POSITION");
            return XxxPath::DontNeedToMove;
        }
        let path = match path.truncate(state, unit_id) {
            Some(path) => path,
            None => {
                println!("try_to_keep_distance: truncate: None");
                return XxxPath::CantFindPath;
            }
        };
        let cost = path.cost_for(state, unit_id);
        let agent = state.parts().agent.get(unit_id);
        if agent.move_points < cost {
            println!("try_to_keep_distance: bad cost");
            return XxxPath::CantFindPath;
        }
        let command = Command::MoveTo(command::MoveTo { id: unit_id, path: path.clone() });
        if check(state, &command).is_ok() {
            println!("try_to_keep_distance: all is fine");
            return XxxPath::Path(path);
        }
        println!("try_to_keep_distance: check err");
        XxxPath::CantFindPath
    }

    fn try_to_find_bad_path(&mut self, state: &State, unit_id: ObjId) -> Option<Command> {
        let path = match self.find_any_path(state, unit_id) {
            Some(path) => path,
            None => return None,
        };
        let path = match path.truncate(state, unit_id) {
            Some(path) => path,
            None => return None,
        };
        let cost = path.cost_for(state, unit_id);
        let agent = state.parts().agent.get(unit_id);
        if agent.move_points < cost {
            return None;
        }
        let command = Command::MoveTo(command::MoveTo { id: unit_id, path });
        if check(state, &command).is_ok() {
            return Some(command);
        }
        None
    }

    fn try_to_move(&mut self, state: &State, unit_id: ObjId) -> Option<Command> {
        // TODO: Don't use type names, check its abilities/components
        let path_result = match state.parts().meta.get(unit_id).name.as_str() {
            "imp" | "imp_toxic" => self.try_to_move_closer(state, unit_id),
            // TODO: Summoner should keep a larger distance
            "imp_bomber" | "imp_summoner" => self.try_to_keep_distance(state, unit_id),
            meta => unimplemented!("unknown agent type: {}", meta),
        };
        // we can't find any good path, so lets find at least something
        // that moves this agent towards enemies
        match path_result {
            XxxPath::Path(path) => {
                let command = Command::MoveTo(command::MoveTo { id: unit_id, path });
                if check(state, &command).is_ok() {
                    Some(command)
                } else {
                    None
                }
            }
            XxxPath::CantFindPath => {
                self.try_to_find_bad_path(state, unit_id)
            }
            XxxPath::DontNeedToMove => None,
        }
    }

    pub fn command(&mut self, state: &State) -> Option<Command> {
        println!("Ai: command...");
        let mut ids = state::players_agent_ids(state, self.id);
        sort_agents_by_distance_to_closest_enemy(state, &mut ids);
        for unit_id in ids {
            if let Some(summon_command) = self.try_summon_imp(state, unit_id) {
                return Some(summon_command);
            }
            if let Some(bomb_command) = self.try_throw_bomb(state, unit_id) {
                return Some(bomb_command);
            }
            if let Some(attack_command) = self.try_to_attack(state, unit_id) {
                return Some(attack_command);
            }
            if let Some(move_command) = self.try_to_move(state, unit_id) {
                return Some(move_command);
            }
        }
        Some(Command::EndTurn(command::EndTurn))
    }
}

// TODO: Move to `state.rs`?
fn sort_agents_by_distance_to_closest_enemy(state: &State, ids: &mut [ObjId]) {
    ids.sort_unstable_by_key(|&id| {
        let agent_player_id = state.parts().belongs_to.get(id).0;
        let agent_pos = state.parts().pos.get(id).0;
        let mut min_distance = state.map().diameter();
        for enemy_id in state::enemy_agent_ids(state, agent_player_id) {
            let enemy_pos = state.parts().pos.get(enemy_id).0;
            let distance = map::distance_hex(agent_pos, enemy_pos);
            if distance < min_distance {
                min_distance = distance;
            }
        }
        min_distance
    });
}
