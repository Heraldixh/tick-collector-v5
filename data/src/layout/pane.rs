use exchange::adapter::PersistStreamKind;
use exchange::{TickMultiplier, TickerInfo};
use serde::{Deserialize, Serialize};

use crate::chart::kline;
use crate::util::ok_or_default;

use crate::chart::{
    Basis, ViewConfig,
    indicator::KlineIndicator,
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Pane {
    Split {
        axis: Axis,
        ratio: f32,
        a: Box<Pane>,
        b: Box<Pane>,
    },
    Starter {
        #[serde(deserialize_with = "ok_or_default", default)]
        link_group: Option<LinkGroup>,
    },
    FootprintChart {
        layout: ViewConfig,
        #[serde(deserialize_with = "ok_or_default", default)]
        stream_type: Vec<PersistStreamKind>,
        #[serde(deserialize_with = "ok_or_default")]
        settings: Settings,
        #[serde(deserialize_with = "ok_or_default", default)]
        indicators: Vec<KlineIndicator>,
        #[serde(deserialize_with = "ok_or_default", default)]
        link_group: Option<LinkGroup>,
    },
}

impl Default for Pane {
    fn default() -> Self {
        Pane::Starter { link_group: None }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Settings {
    pub tick_multiply: Option<exchange::TickMultiplier>,
    pub visual_config: Option<VisualConfig>,
    pub selected_basis: Option<Basis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum LinkGroup {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
}

impl LinkGroup {
    pub const ALL: [LinkGroup; 9] = [
        LinkGroup::A,
        LinkGroup::B,
        LinkGroup::C,
        LinkGroup::D,
        LinkGroup::E,
        LinkGroup::F,
        LinkGroup::G,
        LinkGroup::H,
        LinkGroup::I,
    ];
}

impl std::fmt::Display for LinkGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c = match self {
            LinkGroup::A => "1",
            LinkGroup::B => "2",
            LinkGroup::C => "3",
            LinkGroup::D => "4",
            LinkGroup::E => "5",
            LinkGroup::F => "6",
            LinkGroup::G => "7",
            LinkGroup::H => "8",
            LinkGroup::I => "9",
        };
        write!(f, "{c}")
    }
}

/// Defines the specific configuration for different types of pane settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum VisualConfig {
    Kline(kline::Config),
}

impl VisualConfig {
    pub fn kline(&self) -> Option<kline::Config> {
        match self {
            Self::Kline(cfg) => Some(*cfg),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentKind {
    Starter,
    FootprintChart,
}

impl ContentKind {
    pub const ALL: [ContentKind; 2] = [
        ContentKind::Starter,
        ContentKind::FootprintChart,
    ];
}

impl std::fmt::Display for ContentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ContentKind::Starter => "Starter Pane",
            ContentKind::FootprintChart => "Footprint Chart",
        };
        write!(f, "{s}")
    }
}

#[derive(Clone, Copy)]
pub struct PaneSetup {
    pub ticker_info: exchange::TickerInfo,
    pub basis: Option<Basis>,
    pub tick_multiplier: Option<TickMultiplier>,
    pub tick_size: f32,
    pub depth_aggr: exchange::adapter::StreamTicksize,
    pub push_freq: exchange::PushFrequency,
}

impl PaneSetup {
    pub fn new(
        content_kind: ContentKind,
        base_ticker: TickerInfo,
        _prev_base_ticker: Option<TickerInfo>,
        current_basis: Option<Basis>,
        current_tick_multiplier: Option<TickMultiplier>,
    ) -> Self {
        let exchange = base_ticker.ticker.exchange;

        let basis = match content_kind {
            ContentKind::FootprintChart => {
                // Footprint is tick-only, default to 1000T. Migrate any saved time-based to tick.
                let default_tick = Basis::Tick(crate::aggr::TickCount(1000));
                Some(current_basis
                    .map(|b| if b.is_time() { default_tick } else { b })
                    .unwrap_or(default_tick))
            }
            ContentKind::Starter => None,
        };

        let tick_multiplier = match content_kind {
            ContentKind::FootprintChart => {
                Some(current_tick_multiplier.unwrap_or(TickMultiplier(50)))
            }
            ContentKind::Starter => current_tick_multiplier,
        };

        let tick_size = match tick_multiplier {
            Some(tm) => tm.multiply_with_min_tick_size(base_ticker),
            None => base_ticker.min_ticksize.into(),
        };

        let depth_aggr = exchange.stream_ticksize(tick_multiplier, TickMultiplier(50));

        let push_freq = exchange::PushFrequency::ServerDefault;

        Self {
            ticker_info: base_ticker,
            basis,
            tick_multiplier,
            tick_size,
            depth_aggr,
            push_freq,
        }
    }
}
