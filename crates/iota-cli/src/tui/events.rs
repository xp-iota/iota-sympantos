use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::kanban_view::{KanbanSnapshot, KanbanViewMode};
use super::state::ConversationEntry;
use super::{Overlay, TuiApp};

pub(super) enum LoopSignal {
    Continue,
    Break,
}

impl TuiApp {
    pub(super) async fn on_key_event(&mut self, key: KeyEvent) -> LoopSignal {
        if let Some(signal) = self.handle_approval_overlay_key(key) {
            return signal;
        }
        if let Some(signal) = self.handle_quit_confirm_overlay_key(key) {
            return signal;
        }
        if let Some(signal) = self.handle_kanban_view_key(key) {
            return signal;
        }

        let signal = self.handle_global_key(key).await;
        if matches!(signal, LoopSignal::Break) {
            return signal;
        }

        self.post_key_housekeeping();
        LoopSignal::Continue
    }

    fn handle_approval_overlay_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        self.pending_approval.as_ref()?;
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(LoopSignal::Break),
            (KeyModifiers::NONE, KeyCode::Char('y')) | (KeyModifiers::NONE, KeyCode::Char('Y')) => {
                if let Some(req) = self.pending_approval.take() {
                    let _ = req.reply.send(true);
                    self.record_entry(ConversationEntry::ToolResult {
                        name: req.tool_name,
                        ok: true,
                        text: "approved".to_string(),
                    });
                }
                Some(LoopSignal::Continue)
            }
            (KeyModifiers::NONE, KeyCode::Char('n'))
            | (KeyModifiers::NONE, KeyCode::Char('N'))
            | (KeyModifiers::NONE, KeyCode::Esc) => {
                if let Some(req) = self.pending_approval.take() {
                    let _ = req.reply.send(false);
                    self.record_entry(ConversationEntry::ToolResult {
                        name: req.tool_name,
                        ok: false,
                        text: "denied".to_string(),
                    });
                }
                Some(LoopSignal::Continue)
            }
            _ => Some(LoopSignal::Continue),
        }
    }

    fn handle_quit_confirm_overlay_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        if self.overlay != Overlay::QuitConfirm {
            return None;
        }
        if matches!(
            (key.modifiers, key.code),
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
        ) {
            return Some(LoopSignal::Break);
        }
        self.overlay = Overlay::None;
        self.quit_confirm_tick = None;
        Some(LoopSignal::Continue)
    }

    async fn handle_global_key(&mut self, key: KeyEvent) -> LoopSignal {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.quit_confirm_tick.is_some() {
                    return LoopSignal::Break;
                }
                self.quit_confirm_tick = Some(self.tick_count);
                self.overlay = Overlay::QuitConfirm;
            }
            (KeyModifiers::NONE, KeyCode::Esc) => {
                if self.running_turn {
                    if let Some(handle) = self.turn_task.take() {
                        handle.abort();
                    }
                    self.engine.lock().await.shutdown_open_clients().await;
                    self.running_turn = false;
                    self.streaming_text.clear();
                    self.streaming_version
                        .set(self.streaming_version.get().wrapping_add(1));
                    self.streaming_backend = None;
                    self.turn_started_at = None;
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: "Interrupted".into(),
                    });
                }
                self.overlay = Overlay::None;
            }
            (KeyModifiers::NONE, KeyCode::Char('?')) => {
                self.help_requested = true;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                self.cycle_backend();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.history.entries.clear();
                self.pending_scrollback_entries.clear();
                self.record_entry(ConversationEntry::SystemNotice {
                    text: "Transcript cache cleared (terminal scrollback preserved)".into(),
                });
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => match self.export_history() {
                Ok(path) => {
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: format!("Exported to {}", path.display()),
                    });
                }
                Err(err) => {
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: format!("Export failed: {}", err),
                    });
                }
            },
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if self.slash_tab_complete() {
                    // slash completion accepted — nothing else to do
                } else if self.running_turn && !self.composer.text.trim().is_empty() {
                    self.submit();
                }
            }
            _ => {
                let action = self.composer.handle_key(key);
                if matches!(action, super::input::ComposerAction::Submit) {
                    self.submit();
                }
            }
        }
        LoopSignal::Continue
    }

    fn handle_kanban_view_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        if !self.kanban_view.active {
            return None;
        }
        if matches!(
            (key.modifiers, key.code),
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
        ) {
            return None;
        }
        if self.running_turn && matches!(key.code, KeyCode::Esc) {
            return None;
        }
        if !self.composer.text.is_empty() && !matches!(key.code, KeyCode::Esc) {
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.kanban_view.close();
                return Some(LoopSignal::Continue);
            }
            KeyCode::Char(ch) if key.modifiers == KeyModifiers::NONE => {
                if let Some(mode) = KanbanViewMode::from_digit(ch) {
                    self.kanban_view.mode = mode;
                    return Some(LoopSignal::Continue);
                }
            }
            _ => {}
        }

        let snapshot = match KanbanSnapshot::load(
            self.kanban_store.as_ref(),
            self.kanban_view.board_slug.as_deref(),
        ) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!("Kanban load failed: {}", err),
                });
                return Some(LoopSignal::Continue);
            }
        };

        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Char('v')) => {
                self.kanban_view.cycle_mode();
            }
            (KeyModifiers::SHIFT, KeyCode::Char('L'))
            | (KeyModifiers::NONE, KeyCode::Char('L')) => {
                self.kanban_view.mode = KanbanViewMode::Graph;
            }
            (KeyModifiers::SHIFT, KeyCode::Char('R'))
            | (KeyModifiers::NONE, KeyCode::Char('R')) => {
                self.kanban_view.mode = KanbanViewMode::Timeline;
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.kanban_view.select_column_delta(1, &snapshot);
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) | (KeyModifiers::NONE, KeyCode::BackTab) => {
                self.kanban_view.select_column_delta(-1, &snapshot);
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                self.kanban_view.select_task_delta(1, &snapshot);
            }
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                self.kanban_view.select_task_delta(-1, &snapshot);
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                self.kanban_view.detail_open = !self.kanban_view.detail_open;
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.execute_kanban_command(&format!("dispatch #{id}"));
                }
            }
            (KeyModifiers::SHIFT, KeyCode::Char('D'))
            | (KeyModifiers::NONE, KeyCode::Char('D')) => {
                self.execute_kanban_command("daemon");
            }
            (KeyModifiers::NONE, KeyCode::Char('s')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.execute_kanban_command(&format!("specify #{id}"));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('x')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.execute_kanban_command(&format!("decompose #{id}"));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('m')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.set_composer_text(format!("/kanban move #{id} "));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('e')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.set_composer_text(format!("/kanban edit #{id} title "));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('c')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.set_composer_text(format!("/kanban comment #{id} "));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('a')) => {
                if let Some(id) = self.kanban_view.selected_task_id(&snapshot) {
                    self.set_composer_text(format!("/kanban assign #{id} "));
                }
            }
            (KeyModifiers::NONE, KeyCode::Char('n')) => {
                self.set_composer_text("/kanban create ".to_string());
            }
            (KeyModifiers::NONE, KeyCode::Char('/')) => {
                self.set_composer_text("/kanban filter ".to_string());
            }
            _ => return None,
        }
        Some(LoopSignal::Continue)
    }

    fn set_composer_text(&mut self, text: String) {
        self.composer.cursor = text.chars().count();
        self.composer.text = text;
    }

    fn post_key_housekeeping(&mut self) {
        if self.overlay != Overlay::QuitConfirm {
            self.quit_confirm_tick = None;
        }
        if let Some(t) = self.quit_confirm_tick
            && self.tick_count.saturating_sub(t) > 25
        {
            self.overlay = Overlay::None;
            self.quit_confirm_tick = None;
        }
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod events_tests;
