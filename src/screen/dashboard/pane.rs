use crate::{
    chart::{self, kline::KlineChart},
    modal::{
        self, ModifierKind,
        pane::{
            Modal,
            mini_tickers_list::MiniPanel,
            stack_modal,
        },
    },
    screen::dashboard::tickers_table::TickersTable,
    style::{self, Icon, icon_text},
    widget::{self, button_with_tooltip, column_drag, link_group_button, toast::Toast},
    window::{self, Window},
};
use data::{
    UserTimezone,
    chart::{
        Basis, ViewConfig,
        indicator::{Indicator, KlineIndicator, UiIndicator},
    },
    layout::pane::{ContentKind, LinkGroup, PaneSetup, Settings, VisualConfig},
};
use exchange::{
    Kline, OpenInterest, StreamPairKind, TickMultiplier, TickerInfo, Timeframe,
    adapter::{MarketKind, PersistStreamKind, ResolvedStream, StreamKind, StreamTicksize},
    fetcher::FetchRequests,
};
use iced::{
    Alignment, Element, Length, Renderer, Theme,
    alignment::Vertical,
    padding,
    widget::{button, center, column, container, pane_grid, row, text, tooltip},
};
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum Effect {
    RefreshStreams,
    RequestFetch(FetchRequests),
    SwitchTickersInGroup(TickerInfo),
    FocusWidget(iced::widget::Id),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Status {
    #[default]
    Ready,
    Loading(exchange::fetcher::InfoKind),
    Stale(String),
}

pub enum Action {
    Chart(chart::Action),
    ResolveStreams(Vec<PersistStreamKind>),
    ResolveContent,
}

#[derive(Debug, Clone)]
pub enum Message {
    PaneClicked(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    ClosePane(pane_grid::Pane),
    SplitPane(pane_grid::Axis, pane_grid::Pane),
    MaximizePane(pane_grid::Pane),
    Restore,
    ReplacePane(pane_grid::Pane),
    Popout,
    Merge,
    SwitchLinkGroup(pane_grid::Pane, Option<LinkGroup>),
    VisualConfigChanged(pane_grid::Pane, VisualConfig, bool),
    PaneEvent(pane_grid::Pane, Event),
}

#[derive(Debug, Clone)]
pub enum Event {
    ShowModal(Modal),
    HideModal,
    ContentSelected(ContentKind),
    ChartInteraction(super::chart::Message),
    ToggleIndicator(UiIndicator),
    DeleteNotification(usize),
    ReorderIndicator(column_drag::DragEvent),
    ClusterKindSelected(data::chart::kline::ClusterKind),
    ClusterScalingSelected(data::chart::kline::ClusterScaling),
    StudyConfigurator(modal::pane::settings::study::StudyMessage<data::chart::kline::FootprintStudy>),
    StreamModifierChanged(modal::stream::Message),
    MiniTickersListInteraction(modal::pane::mini_tickers_list::Message),
}

pub struct State {
    id: uuid::Uuid,
    pub modal: Option<Modal>,
    pub content: Content,
    pub settings: Settings,
    pub notifications: Vec<Toast>,
    pub streams: ResolvedStream,
    pub status: Status,
    pub link_group: Option<LinkGroup>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_config(
        content: Content,
        streams: Vec<PersistStreamKind>,
        settings: Settings,
        link_group: Option<LinkGroup>,
    ) -> Self {
        Self {
            content,
            settings,
            streams: ResolvedStream::Waiting(streams),
            link_group,
            ..Default::default()
        }
    }

    pub fn stream_pair(&self) -> Option<TickerInfo> {
        self.streams.find_ready_map(|stream| match stream {
            StreamKind::DepthAndTrades { ticker_info, .. }
            | StreamKind::Kline { ticker_info, .. } => Some(*ticker_info),
        })
    }

    pub fn stream_pair_kind(&self) -> Option<StreamPairKind> {
        let ready_streams = self.streams.ready_iter()?;
        let mut unique = vec![];

        for stream in ready_streams {
            let ticker = stream.ticker_info();
            if !unique.contains(&ticker) {
                unique.push(ticker);
            }
        }

        match unique.len() {
            0 => None,
            1 => Some(StreamPairKind::SingleSource(unique[0])),
            _ => Some(StreamPairKind::MultiSource(unique)),
        }
    }

    pub fn set_content_and_streams(
        &mut self,
        tickers: Vec<TickerInfo>,
        kind: ContentKind,
    ) -> Vec<StreamKind> {
        if !(self.content.kind() == kind) {
            self.settings.selected_basis = None;
            self.settings.tick_multiply = None;
        }

        let base_ticker = tickers[0];
        let prev_base_ticker = self.stream_pair();

        let derived_plan = PaneSetup::new(
            kind,
            base_ticker,
            prev_base_ticker,
            self.settings.selected_basis,
            self.settings.tick_multiply,
        );

        self.settings.selected_basis = derived_plan.basis;
        self.settings.tick_multiply = derived_plan.tick_multiplier;

        let (content, streams) = {
            let kline_stream = |ti: TickerInfo, tf: Timeframe| StreamKind::Kline {
                ticker_info: ti,
                timeframe: tf,
            };
            let depth_stream = |derived_plan: &PaneSetup| StreamKind::DepthAndTrades {
                ticker_info: derived_plan.ticker_info,
                depth_aggr: derived_plan.depth_aggr,
                push_freq: derived_plan.push_freq,
            };

            match kind {
                ContentKind::FootprintChart => {
                    let content = Content::new_footprint(
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        derived_plan.tick_size,
                    );

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M5,
                        |tf| {
                            vec![
                                depth_stream(&derived_plan),
                                kline_stream(derived_plan.ticker_info, tf),
                            ]
                        },
                        || vec![depth_stream(&derived_plan)],
                    );

                    (content, streams)
                }
                ContentKind::Starter => unreachable!(),
            }
        };

        self.content = content;
        self.streams = ResolvedStream::Ready(streams.clone());

        streams
    }

    pub fn insert_hist_oi(&mut self, req_id: Option<uuid::Uuid>, oi: &[OpenInterest]) {
        match &mut self.content {
            Content::Footprint { chart, .. } => {
                let Some(chart) = chart else {
                    log::warn!("Footprint chart wasn't initialized when inserting open interest");
                    return;
                };
                chart.insert_open_interest(req_id, oi);
            }
            _ => {
                log::error!("pane content not footprint chart");
            }
        }
    }

    pub fn insert_hist_klines(
        &mut self,
        req_id: Option<uuid::Uuid>,
        timeframe: Timeframe,
        ticker_info: TickerInfo,
        klines: &[Kline],
    ) {
        match &mut self.content {
            Content::Footprint {
                chart, indicators, ..
            } => {
                let Some(chart) = chart else {
                    log::warn!("chart wasn't initialized when inserting klines");
                    return;
                };

                if let Some(id) = req_id {
                    if chart.basis() != Basis::Time(timeframe) {
                        log::warn!(
                            "Ignoring stale kline fetch for timeframe {:?}; chart basis = {:?}",
                            timeframe,
                            chart.basis()
                        );
                        return;
                    }
                    chart.insert_hist_klines(id, klines);
                } else {
                    let (raw_trades, tick_size) = (chart.raw_trades(), chart.tick_size());
                    let layout = chart.chart_layout();

                    *chart = KlineChart::new(
                        layout,
                        Basis::Time(timeframe),
                        tick_size,
                        klines,
                        raw_trades,
                        indicators,
                        ticker_info,
                        chart.kind(),
                    );
                }
            }
            _ => {
                log::error!("pane content not footprint chart");
            }
        }
    }

    fn has_stream(&self) -> bool {
        match &self.streams {
            ResolvedStream::Ready(streams) => !streams.is_empty(),
            ResolvedStream::Waiting(streams) => !streams.is_empty(),
        }
    }

    pub fn view<'a>(
        &'a self,
        id: pane_grid::Pane,
        panes: usize,
        is_focused: bool,
        maximized: bool,
        window: window::Id,
        main_window: &'a Window,
        timezone: UserTimezone,
        tickers_table: &'a TickersTable,
    ) -> pane_grid::Content<'a, Message, Theme, Renderer> {
        let mut stream_info_element = if Content::Starter == self.content {
            row![]
        } else {
            row![link_group_button(id, self.link_group, |id| {
                Message::PaneEvent(id, Event::ShowModal(Modal::LinkGroup))
            })]
        };

        if let Some(kind) = self.stream_pair_kind() {
            let (base_ti, extra) = match kind {
                StreamPairKind::MultiSource(list) => (list[0], list.len().saturating_sub(1)),
                StreamPairKind::SingleSource(ti) => (ti, 0),
            };

            let exchange_icon = icon_text(style::exchange_icon(base_ti.ticker.exchange), 14);
            let mut label = {
                let symbol = base_ti.ticker.display_symbol_and_type().0;
                match base_ti.ticker.market_type() {
                    MarketKind::Spot => symbol,
                    MarketKind::LinearPerps | MarketKind::InversePerps => symbol + " PERP",
                }
            };
            if extra > 0 {
                label = format!("{label} +{extra}");
            }

            // Display ticker info as text only (no dropdown button)
            let content = row![exchange_icon, text(label).size(14)]
                .align_y(Vertical::Center)
                .spacing(4);

            stream_info_element = stream_info_element.push(content);
        }

        let modifier: Option<modal::stream::Modifier> = self.modal.clone().and_then(|m| {
            if let Modal::StreamModifier(modifier) = m {
                Some(modifier)
            } else {
                None
            }
        });

        let compact_controls = if self.modal == Some(Modal::Controls) {
            Some(
                container(self.view_controls(id, panes, maximized, window != main_window.id))
                    .style(style::chart_modal)
                    .into(),
            )
        } else {
            None
        };

        let uninitialized_base = |kind: ContentKind| -> Element<'a, Message> {
            if self.has_stream() {
                center(text("Loading…").size(16)).into()
            } else {
                let content = column![
                    text(kind.to_string()).size(16),
                    text("No ticker selected").size(14)
                ]
                .spacing(8)
                .align_x(Alignment::Center);

                center(content).into()
            }
        };

        let body = match &self.content {
            Content::Starter => {
                let base: Element<_> = widget::toast::Manager::new(
                    center(
                        text("Select a ticker from the sidebar").size(16),
                    ),
                    &self.notifications,
                    Alignment::End,
                    move |msg| Message::PaneEvent(id, Event::DeleteNotification(msg)),
                )
                .into();

                self.compose_stack_view(
                    base,
                    id,
                    compact_controls,
                    None,
                    tickers_table,
                )
            }
            Content::Footprint {
                chart,
                indicators,
                ..
            } => {
                if let Some(chart) = chart {
                    // Footprint is tick-only, default to 1000T
                    let basis = self.settings.selected_basis
                        .map(|b| if b.is_time() { Basis::Tick(data::aggr::TickCount(1000)) } else { b })
                        .unwrap_or(Basis::Tick(data::aggr::TickCount(1000)));
                    let tick_multiply = self.settings.tick_multiply.unwrap_or(TickMultiplier(10));

                    let kind = ModifierKind::Footprint(basis, tick_multiply);
                    let base_ticksize = tick_multiply.base(chart.tick_size());

                    let exchange = self.stream_pair().as_ref().map(|info| info.ticker.exchange);

                    let modifiers = row![
                        basis_modifier(id, basis, modifier, kind),
                        ticksize_modifier(
                            id,
                            base_ticksize,
                            tick_multiply,
                            modifier,
                            kind,
                            exchange
                        ),
                    ]
                    .spacing(4);

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = chart::view(chart, indicators, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ChartInteraction(message))
                    });

                    self.compose_stack_view(
                        base,
                        id,
                        compact_controls,
                        None,
                        tickers_table,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::FootprintChart);
                    self.compose_stack_view(
                        base,
                        id,
                        compact_controls,
                        None,
                        tickers_table,
                    )
                }
            }
        };

        match &self.status {
            Status::Loading(exchange::fetcher::InfoKind::FetchingKlines) => {
                stream_info_element = stream_info_element.push(text("Fetching Klines..."));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingTrades(count)) => {
                stream_info_element =
                    stream_info_element.push(text(format!("Fetching Trades... {count} fetched")));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingOI) => {
                stream_info_element = stream_info_element.push(text("Fetching Open Interest..."));
            }
            Status::Stale(msg) => {
                stream_info_element = stream_info_element.push(text(msg));
            }
            Status::Ready => {}
        }

        let content = pane_grid::Content::new(body)
            .style(move |theme| style::pane_background(theme, is_focused));

        let controls = {
            let compact_control = container(
                button(text("...").size(13).align_y(Alignment::End))
                    .on_press(Message::PaneEvent(id, Event::ShowModal(Modal::Controls)))
                    .style(move |theme, status| {
                        style::button::transparent(
                            theme,
                            status,
                            self.modal == Some(Modal::Controls),
                        )
                    }),
            )
            .align_y(Alignment::Center)
            .height(Length::Fixed(32.0))
            .padding(4);

            if self.modal == Some(Modal::Controls) {
                pane_grid::Controls::new(compact_control)
            } else {
                pane_grid::Controls::dynamic(
                    self.view_controls(id, panes, maximized, window != main_window.id),
                    compact_control,
                )
            }
        };

        let title_bar = pane_grid::TitleBar::new(
            stream_info_element
                .padding(padding::left(4).top(1))
                .align_y(Vertical::Center)
                .spacing(8)
                .height(Length::Fixed(32.0)),
        )
        .controls(controls)
        .style(style::pane_title_bar);

        content.title_bar(if self.modal.is_none() {
            title_bar
        } else {
            title_bar.always_show_controls()
        })
    }

    pub fn update(&mut self, msg: Event) -> Option<Effect> {
        match msg {
            Event::ShowModal(requested_modal) => {
                return self.show_modal_with_focus(requested_modal);
            }
            Event::HideModal => {
                self.modal = None;
            }
            Event::ContentSelected(kind) => {
                self.content = Content::placeholder(kind);

                if !matches!(kind, ContentKind::Starter) {
                    self.streams = ResolvedStream::Waiting(vec![]);
                    let modal = Modal::MiniTickersList(MiniPanel::new());

                    if let Some(effect) = self.show_modal_with_focus(modal) {
                        return Some(effect);
                    }
                }
            }
            Event::ChartInteraction(msg) => match &mut self.content {
                Content::Footprint { chart: Some(c), .. } => {
                    super::chart::update(c, &msg);
                }
                _ => {}
            },
            Event::ToggleIndicator(ind) => {
                self.content.toggle_indicator(ind);
            }
            Event::DeleteNotification(idx) => {
                if idx < self.notifications.len() {
                    self.notifications.remove(idx);
                }
            }
            Event::ReorderIndicator(e) => {
                self.content.reorder_indicators(&e);
            }
            Event::ClusterKindSelected(kind) => {
                if let Content::Footprint { chart, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_kind(kind);
                }
            }
            Event::ClusterScalingSelected(scaling) => {
                if let Content::Footprint { chart, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_scaling(scaling);
                }
            }
            Event::StudyConfigurator(study_msg) => match study_msg {
                modal::pane::settings::study::StudyMessage::Footprint(m) => {
                    if let Content::Footprint { chart, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_study_configurator(m);
                    }
                }
            },
            Event::StreamModifierChanged(message) => {
                if let Some(Modal::StreamModifier(mut modifier)) = self.modal.take() {
                    let mut effect: Option<Effect> = None;

                    if let Some(action) = modifier.update(message) {
                        match action {
                            modal::stream::Action::TabSelected(tab) => {
                                modifier.tab = tab;
                            }
                            modal::stream::Action::TicksizeSelected(tm) => {
                                modifier.update_kind_with_multiplier(tm);
                                self.settings.tick_multiply = Some(tm);

                                if let Some(ticker) = self.stream_pair() {
                                    if let Content::Footprint { chart: Some(c), .. } = &mut self.content {
                                        c.change_tick_size(
                                            tm.multiply_with_min_tick_size(ticker),
                                        );
                                        c.reset_request_handler();
                                    }
                                }

                                let is_client = self
                                    .stream_pair()
                                    .map(|ti| ti.exchange().is_depth_client_aggr())
                                    .unwrap_or(false);

                                if let Some(mut it) = self.streams.ready_iter_mut() {
                                    for s in &mut it {
                                        if let StreamKind::DepthAndTrades { depth_aggr, .. } = s {
                                            *depth_aggr = if is_client {
                                                StreamTicksize::Client
                                            } else {
                                                StreamTicksize::ServerSide(tm)
                                            };
                                        }
                                    }
                                }
                                if !is_client {
                                    effect = Some(Effect::RefreshStreams);
                                }
                            }
                            modal::stream::Action::BasisSelected(new_basis) => {
                                modifier.update_kind_with_basis(new_basis);
                                self.settings.selected_basis = Some(new_basis);

                                let base_ticker = self.stream_pair();

                                if let Content::Footprint { chart: Some(c), .. } = &mut self.content {
                                    if let Some(base_ticker) = base_ticker {
                                        match new_basis {
                                            Basis::Time(tf) => {
                                                let kline_stream = StreamKind::Kline {
                                                    ticker_info: base_ticker,
                                                    timeframe: tf,
                                                };
                                                let depth_aggr = if base_ticker
                                                    .exchange()
                                                    .is_depth_client_aggr()
                                                {
                                                    StreamTicksize::Client
                                                } else {
                                                    StreamTicksize::ServerSide(
                                                        self.settings
                                                            .tick_multiply
                                                            .unwrap_or(TickMultiplier(1)),
                                                    )
                                                };
                                                let streams = vec![
                                                    kline_stream,
                                                    StreamKind::DepthAndTrades {
                                                        ticker_info: base_ticker,
                                                        depth_aggr,
                                                        push_freq: exchange::PushFrequency::ServerDefault,
                                                    },
                                                ];

                                                self.streams = ResolvedStream::Ready(streams);
                                                let action = c.set_basis(new_basis);

                                                if let Some(chart::Action::RequestFetch(fetch)) = action {
                                                    effect = Some(Effect::RequestFetch(fetch));
                                                }
                                            }
                                            Basis::Tick(_) => {
                                                let depth_aggr = if base_ticker
                                                    .exchange()
                                                    .is_depth_client_aggr()
                                                {
                                                    StreamTicksize::Client
                                                } else {
                                                    StreamTicksize::ServerSide(
                                                        self.settings
                                                            .tick_multiply
                                                            .unwrap_or(TickMultiplier(1)),
                                                    )
                                                };

                                                self.streams = ResolvedStream::Ready(vec![
                                                    StreamKind::DepthAndTrades {
                                                        ticker_info: base_ticker,
                                                        depth_aggr,
                                                        push_freq: exchange::PushFrequency::ServerDefault,
                                                    },
                                                ]);
                                                c.set_basis(new_basis);
                                                effect = Some(Effect::RefreshStreams);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    self.modal = Some(Modal::StreamModifier(modifier));

                    if let Some(e) = effect {
                        return Some(e);
                    }
                }
            }
            Event::MiniTickersListInteraction(message) => {
                if let Some(Modal::MiniTickersList(ref mut mini_panel)) = self.modal
                    && let Some(action) = mini_panel.update(message)
                {
                    self.modal = Some(Modal::MiniTickersList(mini_panel.clone()));

                    let crate::modal::pane::mini_tickers_list::Action::RowSelected(sel) = action;
                    match sel {
                        crate::modal::pane::mini_tickers_list::RowSelection::Switch(ti) => {
                            return Some(Effect::SwitchTickersInGroup(ti));
                        }
                        _ => {}
                    }
                }
            }
        }
        None
    }

    fn view_controls(
        &'_ self,
        pane: pane_grid::Pane,
        total_panes: usize,
        is_maximized: bool,
        is_popout: bool,
    ) -> Element<'_, Message> {
        let control_btn_style = |is_active: bool| {
            move |theme: &Theme, status: button::Status| {
                style::button::transparent(theme, status, is_active)
            }
        };

        let tooltip_pos = tooltip::Position::Bottom;
        let mut buttons = row![];


        if is_popout {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Popout, 12),
                Message::Merge,
                Some("Merge"),
                tooltip_pos,
                control_btn_style(is_popout),
            ));
        } else if total_panes > 1 {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Popout, 12),
                Message::Popout,
                Some("Pop out"),
                tooltip_pos,
                control_btn_style(is_popout),
            ));
        }

        if total_panes > 1 {
            let (resize_icon, message) = if is_maximized {
                (Icon::ResizeSmall, Message::Restore)
            } else {
                (Icon::ResizeFull, Message::MaximizePane(pane))
            };

            buttons = buttons.push(button_with_tooltip(
                icon_text(resize_icon, 12),
                message,
                None,
                tooltip_pos,
                control_btn_style(is_maximized),
            ));

            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Close, 12),
                Message::ClosePane(pane),
                None,
                tooltip_pos,
                control_btn_style(false),
            ));
        }

        buttons
            .padding(padding::right(4).left(4))
            .align_y(Vertical::Center)
            .height(Length::Fixed(32.0))
            .into()
    }

    fn compose_stack_view<'a>(
        &'a self,
        base: Element<'a, Message>,
        pane: pane_grid::Pane,
        compact_controls: Option<Element<'a, Message>>,
        selected_tickers: Option<&'a [TickerInfo]>,
        tickers_table: &'a TickersTable,
    ) -> Element<'a, Message> {
        let base =
            widget::toast::Manager::new(base, &self.notifications, Alignment::End, move |msg| {
                Message::PaneEvent(pane, Event::DeleteNotification(msg))
            })
            .into();

        let on_blur = Message::PaneEvent(pane, Event::HideModal);

        match &self.modal {
            Some(Modal::LinkGroup) => {
                let content = link_group_modal(pane, self.link_group);

                stack_modal(
                    base,
                    content,
                    on_blur,
                    padding::right(12).left(4),
                    Alignment::Start,
                )
            }
            Some(Modal::StreamModifier(modifier)) => stack_modal(
                base,
                modifier.view(self.stream_pair()).map(move |message| {
                    Message::PaneEvent(pane, Event::StreamModifierChanged(message))
                }),
                Message::PaneEvent(pane, Event::HideModal),
                padding::right(12).left(48),
                Alignment::Start,
            ),
            Some(Modal::MiniTickersList(panel)) => {
                let mini_list = panel
                    .view(tickers_table, selected_tickers, self.stream_pair())
                    .map(move |msg| {
                        Message::PaneEvent(pane, Event::MiniTickersListInteraction(msg))
                    });

                let content: Element<_> = container(mini_list)
                    .max_width(260)
                    .padding(16)
                    .style(style::chart_modal)
                    .into();

                stack_modal(
                    base,
                    content,
                    Message::PaneEvent(pane, Event::HideModal),
                    padding::left(12),
                    Alignment::Start,
                )
            }
            Some(Modal::Controls) => stack_modal(
                base,
                if let Some(controls) = compact_controls {
                    controls
                } else {
                    column![].into()
                },
                on_blur,
                padding::left(12),
                Alignment::End,
            ),
            None => base,
        }
    }

    pub fn matches_stream(&self, stream: &StreamKind) -> bool {
        self.streams.matches_stream(stream)
    }

    fn show_modal_with_focus(&mut self, requested_modal: Modal) -> Option<Effect> {
        let should_toggle_close = match (&self.modal, &requested_modal) {
            (Some(Modal::StreamModifier(open)), Modal::StreamModifier(req)) => {
                open.view_mode == req.view_mode
            }
            (Some(open), req) => core::mem::discriminant(open) == core::mem::discriminant(req),
            _ => false,
        };

        if should_toggle_close {
            self.modal = None;
            return None;
        }

        let focus_widget_id = match &requested_modal {
            Modal::MiniTickersList(m) => Some(m.search_box_id.clone()),
            _ => None,
        };

        self.modal = Some(requested_modal);
        focus_widget_id.map(Effect::FocusWidget)
    }

    pub fn invalidate(&mut self, now: Instant) -> Option<Action> {
        match &mut self.content {
            Content::Footprint { chart, .. } => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
            Content::Starter => None,
        }
    }

    pub fn update_interval(&self) -> Option<u64> {
        match &self.content {
            Content::Footprint { .. } => Some(1000),
            Content::Starter => None,
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        self.content.last_tick()
    }

    pub fn tick(&mut self, now: Instant) -> Option<Action> {
        let invalidate_interval: Option<u64> = self.update_interval();
        let last_tick: Option<Instant> = self.last_tick();

        if let Some(streams) = self.streams.waiting_to_resolve()
            && !streams.is_empty()
        {
            return Some(Action::ResolveStreams(streams.to_vec()));
        }

        if !self.content.initialized() {
            return Some(Action::ResolveContent);
        }

        match (invalidate_interval, last_tick) {
            (Some(interval_ms), Some(previous_tick_time)) => {
                if interval_ms > 0 {
                    let interval_duration = std::time::Duration::from_millis(interval_ms);
                    if now.duration_since(previous_tick_time) >= interval_duration {
                        return self.invalidate(now);
                    }
                }
            }
            (Some(interval_ms), None) => {
                if interval_ms > 0 {
                    return self.invalidate(now);
                }
            }
            (None, _) => {}
        }

        None
    }

    pub fn unique_id(&self) -> uuid::Uuid {
        self.id
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            modal: None,
            content: Content::Starter,
            settings: Settings::default(),
            streams: ResolvedStream::Waiting(vec![]),
            notifications: vec![],
            status: Status::Ready,
            link_group: None,
        }
    }
}

#[derive(Default)]
pub enum Content {
    #[default]
    Starter,
    Footprint {
        chart: Option<KlineChart>,
        indicators: Vec<KlineIndicator>,
        layout: data::chart::ViewConfig,
    },
}

impl Content {
    fn new_footprint(
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
    ) -> Self {
        let (prev_indis, prev_layout) = if let Content::Footprint {
            chart,
            indicators,
            layout,
        } = current_content
        {
            (
                Some(indicators.clone()),
                Some(chart.as_ref().map_or(layout.clone(), |c| c.chart_layout())),
            )
        } else {
            (None, None)
        };

        // Footprint is tick-only, default to 1000T. Migrate any saved time-based to tick.
        let basis = settings.selected_basis
            .map(|b| if b.is_time() { Basis::Tick(data::aggr::TickCount(1000)) } else { b })
            .unwrap_or(Basis::Tick(data::aggr::TickCount(1000)));

        let enabled_indicators = {
            let available = KlineIndicator::for_market(ticker_info.market_type());
            prev_indis.map_or_else(
                || vec![KlineIndicator::Volume],
                |indis| {
                    indis
                        .into_iter()
                        .filter(|i| available.contains(i))
                        .collect()
                },
            )
        };

        let splits = {
            let main_chart_split: f32 = 0.8;
            let mut splits_vec = vec![main_chart_split];

            if !enabled_indicators.is_empty() {
                let num_indicators = enabled_indicators.len();

                if num_indicators > 0 {
                    let indicator_total_height_ratio = 1.0 - main_chart_split;
                    let height_per_indicator_pane =
                        indicator_total_height_ratio / num_indicators as f32;

                    let mut current_split_pos = main_chart_split;
                    for _ in 0..(num_indicators - 1) {
                        current_split_pos += height_per_indicator_pane;
                        splits_vec.push(current_split_pos);
                    }
                }
            }
            splits_vec
        };

        let layout = prev_layout
            .filter(|l| l.splits.len() == splits.len())
            .unwrap_or(ViewConfig {
                splits,
                autoscale: Some(data::chart::Autoscale::FitToVisible),
            });

        let footprint_kind = data::chart::KlineChartKind::Footprint {
            clusters: data::chart::kline::ClusterKind::default(),
            scaling: data::chart::kline::ClusterScaling::default(),
            studies: vec![],
        };

        let chart = KlineChart::new(
            layout.clone(),
            basis,
            tick_size,
            &[],
            vec![],
            &enabled_indicators,
            ticker_info,
            &footprint_kind,
        );

        Content::Footprint {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
        }
    }

    fn placeholder(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Starter => Content::Starter,
            ContentKind::FootprintChart => Content::Footprint {
                chart: None,
                indicators: vec![KlineIndicator::Volume],
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        match self {
            Content::Footprint { chart, .. } => Some(chart.as_ref()?.last_update()),
            Content::Starter => None,
        }
    }

    pub fn toggle_indicator(&mut self, indicator: UiIndicator) {
        match (self, indicator) {
            (
                Content::Footprint {
                    chart, indicators, ..
                },
                UiIndicator::Kline(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            _ => {
                log::warn!("indicator toggle on {indicator:?} pane - ignored");
            }
        }
    }

    pub fn reorder_indicators(&mut self, event: &column_drag::DragEvent) {
        match self {
            Content::Footprint { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::Starter => {
                log::warn!("indicator reorder on Starter pane - ignored");
            }
        }
    }

    pub fn change_visual_config(&mut self, config: VisualConfig) {
        match (self, config) {
            (Content::Footprint { .. }, VisualConfig::Kline(_)) => {
                // Kline config not used for footprint currently
            }
            _ => {}
        }
    }

    pub fn studies(&self) -> Option<data::chart::Study> {
        match &self {
            Content::Footprint { chart, .. } => {
                let chart = chart.as_ref()?;
                if let data::chart::KlineChartKind::Footprint { studies, .. } = chart.kind() {
                    Some(data::chart::Study::Footprint(studies.clone()))
                } else {
                    None
                }
            }
            Content::Starter => None,
        }
    }

    pub fn update_studies(&mut self, studies: data::chart::Study) {
        match (self, studies) {
            (Content::Footprint { chart, .. }, data::chart::Study::Footprint(studies)) => {
                if let Some(c) = chart.as_mut() {
                    c.set_studies(studies.clone());
                } else {
                    log::warn!("footprint chart not initialized when updating studies");
                }
            }
            _ => {}
        }
    }

    pub fn kind(&self) -> ContentKind {
        match self {
            Content::Footprint { .. } => ContentKind::FootprintChart,
            Content::Starter => ContentKind::Starter,
        }
    }

    fn initialized(&self) -> bool {
        match self {
            Content::Footprint { chart, .. } => chart.is_some(),
            Content::Starter => true,
        }
    }
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind())
    }
}

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Content::Starter, Content::Starter)
                | (Content::Footprint { .. }, Content::Footprint { .. })
        )
    }
}

fn link_group_modal<'a>(
    pane: pane_grid::Pane,
    selected_group: Option<LinkGroup>,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(4);
    let rows = LinkGroup::ALL.chunks(3);

    for row_groups in rows {
        let mut button_row = row![].spacing(4);

        for &group in row_groups {
            let is_selected = selected_group == Some(group);
            let btn_content = text(group.to_string()).font(style::AZERET_MONO);

            let btn = if is_selected {
                button_with_tooltip(
                    btn_content.align_x(iced::Alignment::Center),
                    Message::SwitchLinkGroup(pane, None),
                    Some("Unlink"),
                    tooltip::Position::Bottom,
                    move |theme, status| style::button::menu_body(theme, status, true),
                )
            } else {
                button(btn_content.align_x(iced::Alignment::Center))
                    .on_press(Message::SwitchLinkGroup(pane, Some(group)))
                    .style(move |theme, status| style::button::menu_body(theme, status, false))
                    .into()
            };

            button_row = button_row.push(btn);
        }

        grid = grid.push(button_row);
    }

    container(grid)
        .max_width(240)
        .padding(16)
        .style(style::chart_modal)
        .into()
}

fn ticksize_modifier<'a>(
    id: pane_grid::Pane,
    base_ticksize: f32,
    multiplier: TickMultiplier,
    modifier: Option<modal::stream::Modifier>,
    kind: ModifierKind,
    exchange: Option<exchange::adapter::Exchange>,
) -> Element<'a, Message> {
    let modifier_modal = Modal::StreamModifier(
        modal::stream::Modifier::new(kind).with_ticksize_view(base_ticksize, multiplier, exchange),
    );

    let is_active = modifier.is_some_and(|m| {
        matches!(
            m.view_mode,
            modal::stream::ViewMode::TicksizeSelection { .. }
        )
    });

    button(text(multiplier.to_string()))
        .style(move |theme, status| style::button::modifier(theme, status, !is_active))
        .on_press(Message::PaneEvent(id, Event::ShowModal(modifier_modal)))
        .into()
}

fn basis_modifier<'a>(
    id: pane_grid::Pane,
    selected_basis: Basis,
    modifier: Option<modal::stream::Modifier>,
    kind: ModifierKind,
) -> Element<'a, Message> {
    let modifier_modal = Modal::StreamModifier(
        modal::stream::Modifier::new(kind).with_view_mode(modal::stream::ViewMode::BasisSelection),
    );

    let is_active =
        modifier.is_some_and(|m| m.view_mode == modal::stream::ViewMode::BasisSelection);

    button(text(selected_basis.to_string()))
        .style(move |theme, status| style::button::modifier(theme, status, !is_active))
        .on_press(Message::PaneEvent(id, Event::ShowModal(modifier_modal)))
        .into()
}

fn by_basis_default<T>(
    basis: Option<Basis>,
    default_tf: Timeframe,
    on_time: impl FnOnce(Timeframe) -> T,
    on_tick: impl FnOnce() -> T,
) -> T {
    match basis.unwrap_or(Basis::Time(default_tf)) {
        Basis::Time(tf) => on_time(tf),
        Basis::Tick(_) => on_tick(),
    }
}
