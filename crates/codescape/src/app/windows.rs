use super::Project;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Win {
    Jump,
    Shortcuts,
    Settings,
    Find,
    Structure,
    Keys,
    MapPanel,
}

impl Win {
    const ALL: [Win; 7] = [Win::Jump, Win::Shortcuts, Win::Settings, Win::Find, Win::Structure, Win::Keys, Win::MapPanel];

    pub(crate) fn id(self) -> egui::Id {
        egui::Id::unique(match self {
            Win::Jump => "Jump to",
            Win::Shortcuts => "Shortcuts",
            Win::Settings => "Project settings",
            Win::Find => "Find in file",
            Win::Structure => "structure",
            Win::Keys => "Keyboard shortcuts",
            Win::MapPanel => "What's on the map",
        })
    }
}

impl Project {
    fn is_open(&self, w: Win) -> bool {
        match w {
            Win::Jump => self.search_open,
            Win::Shortcuts => self.shortcuts_open,
            Win::Settings => self.settings_open,
            Win::Find => self.find.is_some(),
            Win::Structure => self.structure.is_some(),
            Win::Keys => self.keys_open,
            Win::MapPanel => self.map_panel_open,
        }
    }

    fn close(&mut self, w: Win) {
        match w {
            Win::Jump => self.search_open = false,
            Win::Shortcuts => self.shortcuts_open = false,
            Win::Settings => self.settings_open = false,
            Win::Find => self.find = None,
            Win::Structure => self.structure = None,
            Win::Keys => {
                self.keys_open = false;
                self.capturing = None;
            }
            Win::MapPanel => self.map_panel_open = false,
        }
    }

    pub(super) fn track_windows(&mut self, ctx: &egui::Context) {
        let open: Vec<Win> = Win::ALL.into_iter().filter(|&w| self.is_open(w)).collect();
        let top_id = ctx.memory(|m| m.areas().top_layer_id(egui::Order::Middle)).map(|l| l.id);
        let top = Win::ALL.into_iter().find(|w| Some(w.id()) == top_id);
        self.windows.sync(&open, top);
    }

    pub(super) fn close_top_window(&mut self) -> bool {
        let Some(w) = self.windows.pop() else { return false };
        self.close(w);
        true
    }
}
