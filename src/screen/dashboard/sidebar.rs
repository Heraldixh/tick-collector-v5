use super::tickers_table::{self, TickersTable};
use crate::{
    TooltipPosition,
    layout::SavedState,
    style::{self, Icon, icon_text},
    widget::button_with_tooltip,
};
use data::{sidebar, layout::pane::ContentKind};
use exchange::adapter::MarketKind;

use iced::{
    Alignment, Element, Length, Subscription, Task,
    alignment,
    widget::{button, column, container, row, scrollable, space, text, toggler},
};
use rustc_hash::FxHashMap;

const TICKER_ROW_HEIGHT: f32 = 28.0;
const MIN_VOLUME_THRESHOLD: f32 = 50_000_000.0; // 50M daily volume

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketFilter {
    All,
    Spot,
    LinearPerps,    // USDT-margined perpetuals (no expiry)
    InversePerps,   // Coin-margined perpetuals (no expiry)
    AllPerps,       // All perpetual contracts
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    ToggleSidebarMenu(Option<sidebar::Menu>),
    SetSidebarPosition(sidebar::Position),
    TickersTable(super::tickers_table::Message),
    ToggleLargeVolumeFilter(bool),
    SetMarketFilter(MarketFilter),
}

pub struct Sidebar {
    pub state: data::Sidebar,
    pub tickers_table: TickersTable,
    pub large_volume_filter: bool,
    pub market_filter: MarketFilter,
}

pub enum Action {
    TickerSelected(
        exchange::TickerInfo,
        Option<data::layout::pane::ContentKind>,
    ),
    ErrorOccurred(data::InternalError),
}

impl Sidebar {
    pub fn new(state: &SavedState) -> (Self, Task<Message>) {
        let (tickers_table, initial_fetch) =
            if let Some(settings) = state.sidebar.tickers_table.as_ref() {
                TickersTable::new_with_settings(settings)
            } else {
                TickersTable::new()
            };

        (
            Self {
                state: state.sidebar.clone(),
                tickers_table,
                large_volume_filter: true, // Default ON
                market_filter: MarketFilter::All,
            },
            initial_fetch.map(Message::TickersTable),
        )
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<Action>) {
        match message {
            Message::ToggleSidebarMenu(menu) => {
                self.set_menu(menu.filter(|&m| !self.is_menu_active(m)));
            }
            Message::SetSidebarPosition(position) => {
                self.state.position = position;
            }
            Message::ToggleLargeVolumeFilter(enabled) => {
                self.large_volume_filter = enabled;
            }
            Message::SetMarketFilter(filter) => {
                self.market_filter = filter;
            }
            Message::TickersTable(msg) => {
                let action = self.tickers_table.update(msg);

                match action {
                    Some(tickers_table::Action::TickerSelected(ticker_info, content)) => {
                        return (
                            Task::none(),
                            Some(Action::TickerSelected(ticker_info, content)),
                        );
                    }
                    Some(tickers_table::Action::Fetch(task)) => {
                        return (task.map(Message::TickersTable), None);
                    }
                    Some(tickers_table::Action::ErrorOccurred(error)) => {
                        return (Task::none(), Some(Action::ErrorOccurred(error)));
                    }
                    Some(tickers_table::Action::FocusWidget(id)) => {
                        return (iced::widget::operation::focus(id), None);
                    }
                    None => {}
                }
            }
        }

        (Task::none(), None)
    }

    pub fn view(&self, _audio_volume: Option<f32>) -> Element<'_, Message> {
        let state = &self.state;

        let tooltip_position = if state.position == sidebar::Position::Left {
            TooltipPosition::Right
        } else {
            TooltipPosition::Left
        };

        let nav_buttons = self.nav_buttons(false, None, tooltip_position);

        // Create ticker list from available tickers
        let ticker_list = self.view_ticker_list();

        match state.position {
            sidebar::Position::Left => row![nav_buttons, ticker_list],
            sidebar::Position::Right => row![ticker_list, nav_buttons],
        }
        .spacing(4)
        .into()
    }

    fn view_ticker_list(&self) -> Element<'_, Message> {
        let mut ticker_buttons = column![].spacing(2);

        // Build a map of ticker -> volume from ticker_rows
        let volume_map: FxHashMap<_, _> = self.tickers_table.ticker_rows()
            .iter()
            .map(|row| (row.ticker, row.stats.daily_volume))
            .collect();

        // Get sorted list of tickers with filtering
        let mut tickers: Vec<_> = self.tickers_table.tickers_info
            .iter()
            .filter_map(|(ticker, info)| info.as_ref().map(|i| (ticker, i)))
            .filter(|(ticker, info)| {
                // Apply market filter
                let market_type = info.ticker.market_type();
                let market_ok = match self.market_filter {
                    MarketFilter::All => true,
                    MarketFilter::Spot => market_type == MarketKind::Spot,
                    MarketFilter::LinearPerps => market_type == MarketKind::LinearPerps,
                    MarketFilter::InversePerps => market_type == MarketKind::InversePerps,
                    MarketFilter::AllPerps => market_type == MarketKind::LinearPerps || market_type == MarketKind::InversePerps,
                };

                // Apply volume filter
                let volume_ok = if self.large_volume_filter {
                    volume_map.get(ticker).copied().unwrap_or(0.0) >= MIN_VOLUME_THRESHOLD
                } else {
                    true
                };

                market_ok && volume_ok
            })
            .collect();
        
        // Sort by volume (highest first)
        tickers.sort_by(|a, b| {
            let vol_a = volume_map.get(a.0).copied().unwrap_or(0.0);
            let vol_b = volume_map.get(b.0).copied().unwrap_or(0.0);
            vol_b.partial_cmp(&vol_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        for (ticker, info) in tickers {
            let exchange_icon = icon_text(style::exchange_icon(ticker.exchange), 12);
            let (display_str, _market) = info.ticker.display_symbol_and_type();
            
            // Add market suffix (P for perpetuals)
            let market_suffix = match info.ticker.market_type() {
                MarketKind::Spot => "",
                MarketKind::LinearPerps | MarketKind::InversePerps => "P",
            };
            let display_label = format!("{}{}", display_str, market_suffix);

            // Colored bar on left edge (exchange color indicator)
            let color_bar = container(column![])
                .height(Length::Fill)
                .width(Length::Fixed(3.0))
                .style(move |theme| style::ticker_card_bar(theme, 1.0));

            // Toggle switch (default OFF)
            let ticker_copy = *ticker;
            let is_enabled = self.tickers_table.enabled_tickers.contains(ticker);
            let toggle_el = toggler(is_enabled)
                .on_toggle(move |enabled| Message::TickersTable(
                    tickers_table::Message::ToggleTickerEnabled(ticker_copy, enabled)
                ))
                .size(14.0);

            let btn = button(
                row![
                    color_bar,
                    row![exchange_icon, text(display_label).size(12)]
                        .spacing(6)
                        .align_y(alignment::Vertical::Center)
                        .padding(iced::padding::left(6))
                ]
                .align_y(alignment::Vertical::Center)
                .height(Length::Fill)
            )
            .width(Length::Fill)
            .height(Length::Fixed(TICKER_ROW_HEIGHT))
            .style(style::button::ticker_card)
            .on_press(Message::TickersTable(
                tickers_table::Message::TickerSelected(*ticker, Some(ContentKind::FootprintChart))
            ));

            let card = container(
                row![btn, container(toggle_el).padding([0, 12]).align_y(alignment::Vertical::Center)]
                    .align_y(alignment::Vertical::Center)
            )
                .style(style::ticker_card)
                .height(Length::Fixed(TICKER_ROW_HEIGHT))
                .width(Length::Fill);

            ticker_buttons = ticker_buttons.push(card);
        }

        container(
            scrollable(ticker_buttons.padding(iced::padding::right(12)))
                .width(Length::Fill)
                .height(Length::Fill)
        )
        .width(280)
        .height(Length::Fill)
        .padding(4)
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        self.tickers_table.subscription().map(Message::TickersTable)
    }

    fn nav_buttons(
        &self,
        _is_table_open: bool,
        _audio_volume: Option<f32>,
        tooltip_position: TooltipPosition,
    ) -> iced::widget::Column<'_, Message> {
        let settings_modal_button = {
            let is_active = self.is_menu_active(sidebar::Menu::Settings)
                || self.is_menu_active(sidebar::Menu::ThemeEditor);

            button_with_tooltip(
                icon_text(Icon::Cog, 14)
                    .width(24)
                    .align_x(Alignment::Center),
                Message::ToggleSidebarMenu(Some(sidebar::Menu::Settings)),
                None,
                tooltip_position,
                move |theme, status| crate::style::button::transparent(theme, status, is_active),
            )
        };

        column![
            space::vertical(),
            settings_modal_button,
        ]
        .width(32)
        .spacing(8)
    }

    pub fn hide_tickers_table(&mut self) -> bool {
        let table = &mut self.tickers_table;

        if table.expand_ticker_card.is_some() {
            table.expand_ticker_card = None;
            return true;
        } else if table.is_shown {
            table.is_shown = false;
            return true;
        }

        false
    }

    pub fn is_menu_active(&self, menu: sidebar::Menu) -> bool {
        self.state.active_menu == Some(menu)
    }

    pub fn active_menu(&self) -> Option<sidebar::Menu> {
        self.state.active_menu
    }

    pub fn position(&self) -> sidebar::Position {
        self.state.position
    }

    pub fn set_menu(&mut self, menu: Option<sidebar::Menu>) {
        self.state.active_menu = menu;
    }

    pub fn tickers_info(&self) -> &FxHashMap<exchange::Ticker, Option<exchange::TickerInfo>> {
        &self.tickers_table.tickers_info
    }
}
