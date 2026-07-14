//! Plattformneutraler Debouncer: reine Zustandsmaschine `RawSignal -> Trigger`.
//!
//! Enthält **keine** Windows-Aufrufe und **keine** echte Zeitquelle — alle
//! Zeitstempel kommen als `ts_ms`-Parameter herein. Das macht das Modul
//! nativ (auch in der Linux-Sandbox) deterministisch testbar.
//!
//! ## D3-Hinweis
//! `feed` konsumiert ausschließlich [`crate::RawSignal`], das kein Feld für
//! Tasteninhalte besitzt. Der Debouncer zählt Tastendrücke (`key_count`),
//! liest aber nie einen Keycode — es gibt schlicht keinen dafür.

use crate::{RawSignal, Trigger};

/// Default: nach 2 s Tipp-Ruhe gilt ein Burst als abgeschlossen.
pub const DEFAULT_TYPING_PAUSE_MS: u64 = 2000;
/// Default: Klicks, die < 800 ms auseinanderliegen, bilden ein Cluster.
pub const DEFAULT_CLICK_CLUSTER_MS: u64 = 800;
/// Default: nach 500 ms ohne Scroll gilt das Scrollen als beendet.
pub const DEFAULT_SCROLL_END_MS: u64 = 500;
/// Default: ein Fensterfokus unter 300 ms wird als Fokus-Flackern entprellt.
pub const DEFAULT_MIN_FOCUS_MS: u64 = 300;

/// Laufende Tipp-Burst-Aggregation für das aktuell fokussierte Fenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TypingState {
    hwnd: isize,
    /// Zeitstempel des ersten KeyTick im laufenden Burst.
    start_ms: u64,
    /// Zeitstempel des letzten KeyTick im laufenden Burst.
    last_ms: u64,
    key_count: u32,
}

/// Laufende Klick-Cluster-Aggregation für das aktuell fokussierte Fenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClickState {
    hwnd: isize,
    last_ms: u64,
    click_count: u32,
}

/// Laufende Scroll-Aggregation für das aktuell fokussierte Fenster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollState {
    hwnd: isize,
    last_ms: u64,
}

/// Fokus-Zustand: welches Fenster ist aktuell aktiv, und seit wann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusState {
    hwnd: isize,
    since_ms: u64,
}

/// Reine Zustandsmaschine, die Rohsignale zu Triggern verdichtet.
///
/// Konstruiere mit [`Debouncer::new`] (eigene Schwellen) oder
/// [`Debouncer::with_defaults`]. Signale kommen über [`Debouncer::feed`]
/// herein; zeitbasierte Fälligkeit (ohne neues Signal) wird über
/// [`Debouncer::tick`] ausgelöst — der Aufrufer ruft `tick` periodisch mit
/// der aktuellen Uhrzeit auf.
#[derive(Debug, Clone)]
pub struct Debouncer {
    typing_pause_ms: u64,
    click_cluster_ms: u64,
    scroll_end_ms: u64,
    min_focus_ms: u64,

    focus: Option<FocusState>,
    typing: Option<TypingState>,
    clicks: Option<ClickState>,
    scroll: Option<ScrollState>,
}

impl Debouncer {
    /// Erzeugt einen Debouncer mit expliziten Schwellenwerten (in ms).
    pub fn new(typing_pause_ms: u64, click_cluster_ms: u64, scroll_end_ms: u64, min_focus_ms: u64) -> Self {
        Self {
            typing_pause_ms,
            click_cluster_ms,
            scroll_end_ms,
            min_focus_ms,
            focus: None,
            typing: None,
            clicks: None,
            scroll: None,
        }
    }

    /// Erzeugt einen Debouncer mit den dokumentierten Default-Schwellen.
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_TYPING_PAUSE_MS,
            DEFAULT_CLICK_CLUSTER_MS,
            DEFAULT_SCROLL_END_MS,
            DEFAULT_MIN_FOCUS_MS,
        )
    }

    /// Verarbeitet ein Rohsignal und gibt 0..n fällige Trigger zurück.
    ///
    /// Ein einzelnes Signal kann mehrere Trigger auslösen (z. B. ein
    /// Fensterwechsel, der zugleich einen laufenden Tipp-Burst des alten
    /// Fensters abschließt).
    pub fn feed(&mut self, sig: RawSignal) -> Vec<Trigger> {
        let mut out = Vec::new();
        let now = sig.ts_ms();

        // Zeitbasiert fällige Aggregationen zuerst abräumen, damit ein neues
        // Signal nach einer Pause nicht fälschlich an einen alten Burst
        // anschließt.
        self.flush_due(now, &mut out);

        match sig {
            RawSignal::FocusChange { hwnd, ts_ms } => {
                self.handle_focus_change(hwnd, ts_ms, &mut out);
            }
            RawSignal::KeyTick { ts_ms } => {
                self.handle_key_tick(ts_ms);
            }
            RawSignal::MouseClick { ts_ms } => {
                self.handle_mouse_click(ts_ms);
            }
            RawSignal::Scroll { ts_ms } => {
                self.handle_scroll(ts_ms);
            }
        }

        out
    }

    /// Prüft zeitbasierte Fälligkeit ohne ein neues Signal (Tipp-Pause,
    /// Klick-Cluster-Ende, Scroll-Ende). Der Daemon ruft dies periodisch
    /// (z. B. alle 100-200 ms) mit der aktuellen Uhrzeit auf.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Trigger> {
        let mut out = Vec::new();
        self.flush_due(now_ms, &mut out);
        out
    }

    /// Schließt alle Aggregationen ab, deren Ruhe-Schwelle bis `now` erreicht
    /// ist, und hängt die entstehenden Trigger an `out` an.
    fn flush_due(&mut self, now: u64, out: &mut Vec<Trigger>) {
        if let Some(t) = self.typing {
            if now.saturating_sub(t.last_ms) >= self.typing_pause_ms {
                out.push(Trigger::TypingSettled {
                    hwnd: t.hwnd,
                    ts_ms: t.last_ms,
                    key_count: t.key_count,
                    duration_ms: t.last_ms.saturating_sub(t.start_ms),
                });
                self.typing = None;
            }
        }
        if let Some(c) = self.clicks {
            if now.saturating_sub(c.last_ms) >= self.click_cluster_ms {
                out.push(Trigger::ClickCluster {
                    hwnd: c.hwnd,
                    ts_ms: c.last_ms,
                    click_count: c.click_count,
                });
                self.clicks = None;
            }
        }
        if let Some(s) = self.scroll {
            if now.saturating_sub(s.last_ms) >= self.scroll_end_ms {
                out.push(Trigger::ScrollEnd {
                    hwnd: s.hwnd,
                    ts_ms: s.last_ms,
                });
                self.scroll = None;
            }
        }
    }

    /// Schließt (ohne Zeitprüfung) alle laufenden Aggregationen für `hwnd`
    /// sofort ab — verwendet bei Fensterwechsel, damit ein Burst des alten
    /// Fensters nicht fälschlich dem neuen Fenster zugeschrieben wird.
    fn flush_all_for_old_window(&mut self, out: &mut Vec<Trigger>) {
        if let Some(t) = self.typing.take() {
            out.push(Trigger::TypingSettled {
                hwnd: t.hwnd,
                ts_ms: t.last_ms,
                key_count: t.key_count,
                duration_ms: t.last_ms.saturating_sub(t.start_ms),
            });
        }
        if let Some(c) = self.clicks.take() {
            out.push(Trigger::ClickCluster {
                hwnd: c.hwnd,
                ts_ms: c.last_ms,
                click_count: c.click_count,
            });
        }
        if let Some(s) = self.scroll.take() {
            out.push(Trigger::ScrollEnd {
                hwnd: s.hwnd,
                ts_ms: s.last_ms,
            });
        }
    }

    fn handle_focus_change(&mut self, hwnd: isize, ts_ms: u64, out: &mut Vec<Trigger>) {
        // Laufende Aggregation dem alten Fenster zuschreiben und flushen,
        // unabhängig davon, ob der Fokuswechsel selbst entprellt wird.
        self.flush_all_for_old_window(out);

        match self.focus {
            Some(prev) => {
                let was_long_enough = ts_ms.saturating_sub(prev.since_ms) >= self.min_focus_ms;
                if was_long_enough && prev.hwnd != hwnd {
                    out.push(Trigger::FocusChange { hwnd, ts_ms });
                }
                // Sehr kurze vorherige Fokuszeiten (Flackern) werden
                // entprellt: kein FocusChange-Trigger, aber der interne
                // Zustand wird trotzdem auf das neue Fenster gesetzt.
                if prev.hwnd != hwnd {
                    self.focus = Some(FocusState { hwnd, since_ms: ts_ms });
                }
            }
            None => {
                // Erster Fokus überhaupt: sofort melden.
                out.push(Trigger::FocusChange { hwnd, ts_ms });
                self.focus = Some(FocusState { hwnd, since_ms: ts_ms });
            }
        }
    }

    /// Liefert das aktuell fokussierte Fensterhandle, oder `0` als Sentinel
    /// für „noch kein `FocusChange`-Signal gesehen" (Windows vergibt HWND 0
    /// nie an ein echtes Fenster, daher kollisionsfrei).
    fn current_hwnd(&self) -> isize {
        self.focus.map(|f| f.hwnd).unwrap_or(0)
    }

    fn handle_key_tick(&mut self, ts_ms: u64) {
        let hwnd = self.current_hwnd();
        match &mut self.typing {
            Some(t) if t.hwnd == hwnd => {
                t.last_ms = ts_ms;
                t.key_count += 1;
            }
            _ => {
                self.typing = Some(TypingState {
                    hwnd,
                    start_ms: ts_ms,
                    last_ms: ts_ms,
                    key_count: 1,
                });
            }
        }
    }

    fn handle_mouse_click(&mut self, ts_ms: u64) {
        let hwnd = self.current_hwnd();
        match &mut self.clicks {
            Some(c) if c.hwnd == hwnd => {
                c.last_ms = ts_ms;
                c.click_count += 1;
            }
            _ => {
                self.clicks = Some(ClickState {
                    hwnd,
                    last_ms: ts_ms,
                    click_count: 1,
                });
            }
        }
    }

    fn handle_scroll(&mut self, ts_ms: u64) {
        let hwnd = self.current_hwnd();
        match &mut self.scroll {
            Some(s) if s.hwnd == hwnd => {
                s.last_ms = ts_ms;
            }
            _ => {
                self.scroll = Some(ScrollState { hwnd, last_ms: ts_ms });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(hwnd: isize, ts_ms: u64) -> RawSignal {
        RawSignal::FocusChange { hwnd, ts_ms }
    }
    fn key(ts_ms: u64) -> RawSignal {
        RawSignal::KeyTick { ts_ms }
    }
    fn click(ts_ms: u64) -> RawSignal {
        RawSignal::MouseClick { ts_ms }
    }
    fn scroll(ts_ms: u64) -> RawSignal {
        RawSignal::Scroll { ts_ms }
    }

    #[test]
    fn first_focus_change_is_reported_immediately() {
        let mut d = Debouncer::with_defaults();
        let trig = d.feed(focus(1, 1_000));
        assert_eq!(trig, vec![Trigger::FocusChange { hwnd: 1, ts_ms: 1_000 }]);
    }

    #[test]
    fn typing_burst_settles_after_pause_via_feed() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        assert!(d.feed(key(100)).is_empty());
        assert!(d.feed(key(300)).is_empty());
        assert!(d.feed(key(500)).is_empty());
        // 500 + 2000 = 2500. Ein Signal bei genau 2500 muss den Burst zuerst
        // flushen (>= Schwelle), bevor es selbst verarbeitet wird.
        let trig = d.feed(click(2_500));
        assert_eq!(
            trig,
            vec![Trigger::TypingSettled {
                hwnd: 1,
                ts_ms: 500,
                key_count: 3,
                duration_ms: 400,
            }]
        );
    }

    #[test]
    fn typing_burst_settles_via_tick_without_new_signal() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(key(100));
        d.feed(key(200));
        assert!(d.tick(1_000).is_empty());
        let trig = d.tick(2_200); // 200 + 2000 = 2200
        assert_eq!(
            trig,
            vec![Trigger::TypingSettled {
                hwnd: 1,
                ts_ms: 200,
                key_count: 2,
                duration_ms: 100,
            }]
        );
        // Nach dem Flush darf derselbe Trigger nicht erneut ausgelöst werden.
        assert!(d.tick(10_000).is_empty());
    }

    #[test]
    fn multiple_bursts_produce_separate_triggers() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(key(0));
        d.feed(key(50));
        let first = d.tick(2_050);
        assert_eq!(
            first,
            vec![Trigger::TypingSettled {
                hwnd: 1,
                ts_ms: 50,
                key_count: 2,
                duration_ms: 50,
            }]
        );

        // Neuer Burst beginnt.
        d.feed(key(3_000));
        d.feed(key(3_100));
        d.feed(key(3_150));
        let second = d.tick(5_150);
        assert_eq!(
            second,
            vec![Trigger::TypingSettled {
                hwnd: 1,
                ts_ms: 3_150,
                key_count: 3,
                duration_ms: 150,
            }]
        );
    }

    #[test]
    fn click_cluster_settles_after_rest() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(click(0));
        d.feed(click(200));
        d.feed(click(400));
        assert!(d.tick(1_000).is_empty()); // 400 + 800 = 1200, noch nicht fällig
        let trig = d.tick(1_200);
        assert_eq!(
            trig,
            vec![Trigger::ClickCluster {
                hwnd: 1,
                ts_ms: 400,
                click_count: 3,
            }]
        );
    }

    #[test]
    fn scroll_end_settles_after_rest() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(scroll(0));
        d.feed(scroll(100));
        d.feed(scroll(300));
        assert!(d.tick(700).is_empty()); // 300 + 500 = 800
        let trig = d.tick(800);
        assert_eq!(trig, vec![Trigger::ScrollEnd { hwnd: 1, ts_ms: 300 }]);
    }

    #[test]
    fn focus_change_flushes_running_typing_aggregation_of_old_window() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(key(100));
        d.feed(key(200));

        // Fensterwechsel bei t=1000 (Fenster 1 war lange genug fokussiert).
        let trig = d.feed(focus(2, 1_000));
        assert_eq!(
            trig,
            vec![
                Trigger::TypingSettled {
                    hwnd: 1,
                    ts_ms: 200,
                    key_count: 2,
                    duration_ms: 100,
                },
                Trigger::FocusChange { hwnd: 2, ts_ms: 1_000 },
            ]
        );
    }

    #[test]
    fn focus_change_flushes_running_click_and_scroll_of_old_window() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(click(50));
        d.feed(scroll(60));

        let trig = d.feed(focus(2, 1_000));
        assert_eq!(
            trig,
            vec![
                Trigger::ClickCluster { hwnd: 1, ts_ms: 50, click_count: 1 },
                Trigger::ScrollEnd { hwnd: 1, ts_ms: 60 },
                Trigger::FocusChange { hwnd: 2, ts_ms: 1_000 },
            ]
        );
    }

    #[test]
    fn very_short_focus_is_debounced() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        // Fenster 1 nur 100 ms aktiv (< min_focus_ms = 300) bevor Wechsel zu 2.
        let trig = d.feed(focus(2, 100));
        assert!(
            trig.is_empty(),
            "kurzer Fokuswechsel darf keinen FocusChange-Trigger erzeugen, war: {trig:?}"
        );

        // Danach zählt Fenster 2 als aktuell fokussiert; ein Wechsel nach
        // ausreichender Zeit muss wieder normal gemeldet werden.
        let trig2 = d.feed(focus(3, 500));
        assert_eq!(trig2, vec![Trigger::FocusChange { hwnd: 3, ts_ms: 500 }]);
    }

    #[test]
    fn focus_change_to_same_hwnd_is_noop() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        let trig = d.feed(focus(1, 1_000));
        assert!(trig.is_empty());
    }

    #[test]
    fn key_count_accumulates_correctly_across_many_ticks() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        for i in 0..10u64 {
            d.feed(key(i * 100));
        }
        // letzter KeyTick bei 900; Pause bis 900+2000=2900
        let trig = d.tick(2_900);
        assert_eq!(
            trig,
            vec![Trigger::TypingSettled {
                hwnd: 1,
                ts_ms: 900,
                key_count: 10,
                duration_ms: 900,
            }]
        );
    }

    #[test]
    fn custom_thresholds_are_respected() {
        let mut d = Debouncer::new(500, 100, 50, 10);
        d.feed(focus(1, 0));
        d.feed(key(0));
        assert!(d.tick(400).is_empty());
        let trig = d.tick(500);
        assert_eq!(
            trig,
            vec![Trigger::TypingSettled {
                hwnd: 1,
                ts_ms: 0,
                key_count: 1,
                duration_ms: 0,
            }]
        );
    }

    #[test]
    fn tick_without_any_signals_is_noop() {
        let mut d = Debouncer::with_defaults();
        assert!(d.tick(0).is_empty());
        assert!(d.tick(1_000_000).is_empty());
    }

    #[test]
    fn focus_change_at_exact_min_focus_threshold_is_not_debounced() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        // Exakt 300 ms (== min_focus_ms) gilt als "lange genug" (>=).
        let trig = d.feed(focus(2, DEFAULT_MIN_FOCUS_MS));
        assert_eq!(
            trig,
            vec![Trigger::FocusChange { hwnd: 2, ts_ms: DEFAULT_MIN_FOCUS_MS }]
        );
    }

    #[test]
    fn chain_of_short_focus_flickers_is_fully_suppressed_until_real_switch() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        // Drei kurze Flackerwechsel (je < 300 ms) hintereinander.
        assert!(d.feed(focus(2, 50)).is_empty());
        assert!(d.feed(focus(3, 90)).is_empty());
        assert!(d.feed(focus(4, 120)).is_empty());
        // Danach bleibt Fenster 4 lange genug fokussiert -> normaler Trigger.
        let trig = d.feed(focus(5, 500));
        assert_eq!(trig, vec![Trigger::FocusChange { hwnd: 5, ts_ms: 500 }]);
    }

    #[test]
    fn signals_before_first_focus_change_use_null_hwnd_sentinel() {
        let mut d = Debouncer::with_defaults();
        // Kein FocusChange bisher -> Aggregation läuft unter hwnd=0.
        d.feed(key(0));
        d.feed(key(100));
        let trig = d.tick(2_100);
        assert_eq!(
            trig,
            vec![Trigger::TypingSettled {
                hwnd: 0,
                ts_ms: 100,
                key_count: 2,
                duration_ms: 100,
            }]
        );
    }

    #[test]
    fn independent_aggregations_can_settle_in_same_tick() {
        let mut d = Debouncer::with_defaults();
        d.feed(focus(1, 0));
        d.feed(key(0));
        d.feed(click(0));
        d.feed(scroll(0));
        // scroll_end=500 fires first at 500, click_cluster=800 at 800,
        // typing_pause=2000 at 2000. All should have fired by t=2000.
        let trig = d.tick(2_000);
        assert_eq!(trig.len(), 3);
        assert!(trig.iter().any(|t| matches!(t, Trigger::ScrollEnd { .. })));
        assert!(trig.iter().any(|t| matches!(t, Trigger::ClickCluster { .. })));
        assert!(trig.iter().any(|t| matches!(t, Trigger::TypingSettled { .. })));
    }
}
