/**
 * Tick Collector Web Dashboard
 * Real-time cryptocurrency charting with WebSocket streaming
 * 100% Feature Parity with Desktop App
 */

// State management
const state = {
    tickers: [],
    panes: Array(9).fill(null), // [{ ticker, chart, ws, reconnectTimer, tickCount }, ...]
    filters: {
        largeVolumeOnly: true,  // Default ON
        marketType: 'perps',     // Default All Perps
        exchange: 'all',         // Default All exchanges
        searchQuery: ''
    },
    favorites: new Set(),
    sortBy: 'symbol',
    config: null,
    connectionStatus: 'connected',
    tickAggregation: 1000, // Default: 1000 ticks per candle
    activeTickCountPane: null, // Which pane the tick count modal is for
    activeTickSizePane: null, // Which pane the tick size modal is for
};

// API base URL - detect if running through proxy and use correct WebSocket URL
const API_BASE = window.location.origin;
// For WebSocket, we need to connect to the actual server, not the proxy
// If port is not 8080, assume we're behind a proxy and connect to localhost:8080
const WS_BASE = window.location.port === '8080' 
    ? `ws://${window.location.host}`
    : `ws://localhost:8080`;

// Constants
const RECONNECT_DELAY = 5000;
const TICK_AGGREGATION_OPTIONS = [100, 500, 1000, 2000, 5000];

// Initialize app
document.addEventListener('DOMContentLoaded', async () => {
    console.log('🚀 Tick Collector Web starting...');
    
    // Load favorites from localStorage
    loadFavorites();
    
    // Load config
    await loadConfig();
    
    // Load tickers
    await loadTickers();
    
    // Setup event listeners
    setupEventListeners();
    
    // Restore saved pane assignments
    restorePanes();
    
    // Update ticker count
    updateTickerCount();
    
    console.log('✅ App initialized');
});

// Load favorites from localStorage
function loadFavorites() {
    try {
        const saved = localStorage.getItem('tickCollectorFavorites');
        if (saved) {
            state.favorites = new Set(JSON.parse(saved));
        }
    } catch (e) {
        console.error('Failed to load favorites:', e);
    }
}

// Save favorites to localStorage
function saveFavorites() {
    localStorage.setItem('tickCollectorFavorites', JSON.stringify([...state.favorites]));
}

// Toggle favorite status
function toggleFavorite(tickerKey) {
    if (state.favorites.has(tickerKey)) {
        state.favorites.delete(tickerKey);
    } else {
        state.favorites.add(tickerKey);
    }
    saveFavorites();
    renderTickerList();
    renderFavorites();
}

// Render favorites section (matching desktop app format)
function renderFavorites() {
    const container = document.getElementById('favoritesList');
    const section = document.getElementById('favoritesSection');
    
    if (state.favorites.size === 0) {
        section.classList.add('hidden');
        return;
    }
    
    section.classList.remove('hidden');
    container.innerHTML = '';
    
    const favoriteTickers = state.tickers.filter(t => {
        const key = `${t.exchange}:${t.symbol}`;
        return state.favorites.has(key);
    });
    
    favoriteTickers.forEach(ticker => {
        const key = `${ticker.exchange}:${ticker.symbol}`;
        const isActive = state.panes.some(p => p?.ticker === key);
        const exchangeIcon = getExchangeIcon(ticker.exchange);
        
        const item = document.createElement('div');
        item.className = `ticker-item ${isActive ? 'active' : ''}`;
        item.dataset.ticker = key;
        
        item.innerHTML = `
            <span class="ticker-icon ${ticker.exchange}">${exchangeIcon}</span>
            <span class="ticker-name">${ticker.symbol}</span>
            <div class="ticker-toggle ${isActive ? 'active' : ''}" data-ticker="${key}"></div>
        `;
        
        container.appendChild(item);
    });
}

// Update ticker count display
function updateTickerCount() {
    const countEl = document.getElementById('tickerCount');
    const filtered = filterTickers(state.tickers);
    countEl.textContent = filtered.length;
}

// Format price for display
function formatPrice(price) {
    if (!price || price === 0) return '-';
    if (price >= 1000) return price.toLocaleString('en-US', { maximumFractionDigits: 0 });
    if (price >= 1) return price.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    return price.toLocaleString('en-US', { minimumFractionDigits: 4, maximumFractionDigits: 6 });
}

// Update connection status indicator
function updateConnectionStatus(status) {
    state.connectionStatus = status;
    const el = document.getElementById('connectionStatus');
    el.className = `connection-status ${status}`;
    el.querySelector('.status-text').textContent = 
        status === 'connected' ? 'Connected' :
        status === 'reconnecting' ? 'Reconnecting...' : 'Disconnected';
}

// Load configuration from server
async function loadConfig() {
    try {
        const response = await fetch(`${API_BASE}/api/v1/config`);
        state.config = await response.json();
        
        // Apply saved filters, but keep defaults if not set
        // Default: largeVolumeOnly=true, marketType='perps'
        if (state.config.filters?.large_volume_only !== undefined) {
            state.filters.largeVolumeOnly = state.config.filters.large_volume_only;
        }
        if (state.config.filters?.market_type) {
            state.filters.marketType = state.config.filters.market_type;
        }
        
        document.getElementById('largeVolumeOnly').checked = state.filters.largeVolumeOnly;
        updateFilterButtons();
    } catch (e) {
        console.error('Failed to load config:', e);
        // Keep defaults on error
        document.getElementById('largeVolumeOnly').checked = state.filters.largeVolumeOnly;
        updateFilterButtons();
    }
}

// Load available tickers
async function loadTickers() {
    try {
        const response = await fetch(`${API_BASE}/api/v1/tickers`);
        state.tickers = await response.json();
        renderTickerList();
    } catch (e) {
        console.error('Failed to load tickers:', e);
        // Use default tickers for demo
        state.tickers = [
            { exchange: 'binance', symbol: 'BTCUSDT', price: 0 },
            { exchange: 'binance', symbol: 'ETHUSDT', price: 0 },
            { exchange: 'binance', symbol: 'SOLUSDT', price: 0 },
            { exchange: 'binance', symbol: 'XRPUSDT', price: 0 },
            { exchange: 'binance', symbol: 'DOGEUSDT', price: 0 },
        ];
        renderTickerList();
    }
}

// Render ticker list in sidebar
function renderTickerList() {
    const container = document.getElementById('tickerList');
    container.innerHTML = '';
    
    let filteredTickers = filterTickers(state.tickers);
    filteredTickers = sortTickers(filteredTickers);
    
    filteredTickers.forEach(ticker => {
        const key = `${ticker.exchange}:${ticker.symbol}`;
        const isActive = state.panes.some(p => p?.ticker === key);
        
        // Get exchange icon character (matching desktop app)
        const exchangeIcon = getExchangeIcon(ticker.exchange);
        
        const item = document.createElement('div');
        item.className = `ticker-item ${isActive ? 'active' : ''}`;
        item.dataset.ticker = key;
        
        // Match desktop format: icon + symbol + toggle
        item.innerHTML = `
            <span class="ticker-icon ${ticker.exchange}">${exchangeIcon}</span>
            <span class="ticker-name">${ticker.symbol}</span>
            <div class="ticker-toggle ${isActive ? 'active' : ''}" data-ticker="${key}"></div>
        `;
        
        container.appendChild(item);
    });
    
    updateTickerCount();
}

// Get exchange icon character (matching desktop app icons exactly)
function getExchangeIcon(exchange) {
    // Icons match the desktop app exactly
    switch (exchange) {
        case 'binance': return '◆';  // Filled diamond (yellow)
        case 'bybit': return '✦';    // 4-pointed star (orange)
        case 'okx': return '▣';      // Square with inner square (white)
        case 'hyperliquid': return '◆'; // Filled diamond (green)
        default: return '●';
    }
}

// Sort tickers based on current sort option
function sortTickers(tickers) {
    const sorted = [...tickers];
    
    // When Large volume only is ON, sort by volume (highest to lowest)
    if (state.filters.largeVolumeOnly) {
        sorted.sort((a, b) => (b.volume_24h || 0) - (a.volume_24h || 0));
        return sorted;
    }
    
    // Otherwise sort alphabetically by symbol
    sorted.sort((a, b) => a.symbol.localeCompare(b.symbol));
    return sorted;
}

// Filter tickers based on current filters
function filterTickers(tickers) {
    return tickers.filter(t => {
        // Search filter
        if (state.filters.searchQuery) {
            const query = state.filters.searchQuery.toUpperCase();
            if (!t.symbol.toUpperCase().includes(query) && 
                !t.base_asset?.toUpperCase().includes(query)) {
                return false;
            }
        }
        
        // Exchange filter
        if (state.filters.exchange !== 'all') {
            if (t.exchange !== state.filters.exchange) return false;
        }
        
        // Market type filter
        if (state.filters.marketType !== 'all') {
            const isPerp = t.symbol.includes('PERP') || t.symbol.includes('SWAP') || 
                           !t.symbol.endsWith('USDT') || t.exchange !== 'binance';
            if (state.filters.marketType === 'spot' && isPerp) return false;
            if (state.filters.marketType === 'perps' && !isPerp) return false;
        }
        
        // Large volume filter (placeholder - needs volume data)
        if (state.filters.largeVolumeOnly && t.volume_24h && t.volume_24h < 50000000) {
            return false;
        }
        
        return true;
    });
}

// Setup event listeners
function setupEventListeners() {
    // Ticker toggle clicks
    document.getElementById('tickerList').addEventListener('click', (e) => {
        const toggle = e.target.closest('.ticker-toggle');
        if (toggle) {
            const ticker = toggle.dataset.ticker;
            toggleTicker(ticker);
            return;
        }
        
        // Favorite star clicks
        const favorite = e.target.closest('.ticker-favorite');
        if (favorite) {
            const ticker = favorite.dataset.ticker;
            toggleFavorite(ticker);
            return;
        }
    });
    
    // Favorites list clicks
    document.getElementById('favoritesList').addEventListener('click', (e) => {
        const toggle = e.target.closest('.ticker-toggle');
        if (toggle) {
            const ticker = toggle.dataset.ticker;
            toggleTicker(ticker);
        }
    });
    
    // Search input
    document.getElementById('tickerSearch').addEventListener('input', (e) => {
        state.filters.searchQuery = e.target.value;
        renderTickerList();
    });
    
    // Sort select (removed from UI to match desktop)
    
    // Filter checkbox
    document.getElementById('largeVolumeOnly').addEventListener('change', (e) => {
        state.filters.largeVolumeOnly = e.target.checked;
        renderTickerList();
        saveConfig();
    });
    
    // Exchange filter buttons
    document.querySelectorAll('.exchange-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const exchange = btn.dataset.exchange;
            state.filters.exchange = exchange;
            updateExchangeFilterButtons();
            renderTickerList();
            saveConfig();
        });
    });
    
    // Market type filter buttons
    document.querySelectorAll('.filter-btn:not(.exchange-btn)').forEach(btn => {
        btn.addEventListener('click', () => {
            const filter = btn.dataset.filter;
            if (!filter) return;
            state.filters.marketType = filter;
            updateFilterButtons();
            renderTickerList();
            saveConfig();
        });
    });
    
    // Pane close buttons
    document.querySelectorAll('.pane-close').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const pane = e.target.closest('.chart-pane');
            const paneIndex = parseInt(pane.dataset.pane);
            clearPane(paneIndex);
        });
    });
    
    // Tick count buttons - open modal
    document.querySelectorAll('.tick-count-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const paneIndex = parseInt(btn.dataset.pane);
            openTickCountModal(paneIndex);
        });
    });
    
    // Tick count modal - preset buttons
    document.querySelectorAll('.tick-preset-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const ticks = parseInt(btn.dataset.ticks);
            setTickCount(ticks);
            
            // Update active state
            document.querySelectorAll('.tick-preset-btn').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            
            // Clear custom input
            document.getElementById('customTickCount').value = '';
        });
    });
    
    // Tick count modal - custom input
    document.getElementById('customTickCount').addEventListener('change', (e) => {
        let value = parseInt(e.target.value);
        if (value >= 4 && value <= 1000) {
            setTickCount(value);
            // Clear preset active states
            document.querySelectorAll('.tick-preset-btn').forEach(b => b.classList.remove('active'));
        }
    });
    
    // Close modal on overlay click
    document.getElementById('tickCountModal').addEventListener('click', (e) => {
        if (e.target.id === 'tickCountModal') {
            closeTickCountModal();
        }
    });
    
    // Tick size multiplier buttons - open modal
    document.querySelectorAll('.tick-size-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const paneIndex = parseInt(btn.dataset.pane);
            openTickSizeModal(paneIndex);
        });
    });
    
    // Tick size modal - preset buttons
    document.querySelectorAll('.tick-size-preset-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const multiplier = parseInt(btn.dataset.multiplier);
            setTickSizeMultiplier(multiplier);
            
            // Update active state
            document.querySelectorAll('.tick-size-preset-btn').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            
            // Clear custom input
            document.getElementById('customTickSize').value = '';
        });
    });
    
    // Tick size modal - custom input
    document.getElementById('customTickSize').addEventListener('change', (e) => {
        let value = parseInt(e.target.value);
        if (value >= 1 && value <= 2000) {
            setTickSizeMultiplier(value);
            // Clear preset active states
            document.querySelectorAll('.tick-size-preset-btn').forEach(b => b.classList.remove('active'));
        }
    });
    
    // Close tick size modal on overlay click
    document.getElementById('tickSizeModal').addEventListener('click', (e) => {
        if (e.target.id === 'tickSizeModal') {
            closeTickSizeModal();
        }
    });
    
    // Pane maximize buttons
    document.querySelectorAll('.pane-btn[title="Maximize"]').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const pane = e.target.closest('.chart-pane');
            pane.classList.toggle('maximized');
            
            // Resize chart
            const paneIndex = parseInt(pane.dataset.pane);
            if (state.panes[paneIndex]?.chart) {
                setTimeout(() => {
                    const container = document.getElementById(`chart-${paneIndex}`);
                    state.panes[paneIndex].chart.resize(
                        container.clientWidth,
                        container.clientHeight
                    );
                }, 100);
            }
        });
    });
    
    // Window resize
    window.addEventListener('resize', () => {
        state.panes.forEach((pane, i) => {
            if (pane?.chart) {
                const container = document.getElementById(`chart-${i}`);
                pane.chart.resize(container.clientWidth, container.clientHeight);
            }
        });
    });
}

// Update filter button states
function updateFilterButtons() {
    document.querySelectorAll('.filter-btn:not(.exchange-btn)').forEach(btn => {
        if (btn.dataset.filter) {
            btn.classList.toggle('active', btn.dataset.filter === state.filters.marketType);
        }
    });
}

// Open tick count modal for a specific pane
function openTickCountModal(paneIndex) {
    state.activeTickCountPane = paneIndex;
    const pane = state.panes[paneIndex];
    const currentTicks = pane?.footprintState?.tickCount || 1000;
    
    // Update modal to show current selection
    document.querySelectorAll('.tick-preset-btn').forEach(btn => {
        btn.classList.toggle('active', parseInt(btn.dataset.ticks) === currentTicks);
    });
    document.getElementById('customTickCount').value = '';
    
    document.getElementById('tickCountModal').classList.add('active');
}

// Close tick count modal
function closeTickCountModal() {
    state.activeTickCountPane = null;
    document.getElementById('tickCountModal').classList.remove('active');
}

// Set tick count for the active pane
function setTickCount(ticks) {
    const paneIndex = state.activeTickCountPane;
    if (paneIndex === null) return;
    
    const pane = state.panes[paneIndex];
    if (pane && pane.footprintState) {
        // Update tick count
        pane.footprintState.tickCount = ticks;
        
        // Re-aggregate ALL existing trades with new tick count
        if (pane.footprintState.reAggregateAllTrades) {
            pane.footprintState.reAggregateAllTrades();
        }
        
        // Update button text
        const tickBtn = document.querySelector(`.tick-count-btn[data-pane="${paneIndex}"]`);
        if (tickBtn) tickBtn.textContent = `${ticks}T`;
        
        // Update pane title
        const paneEl = document.querySelector(`.chart-pane[data-pane="${paneIndex}"]`);
        const symbol = pane.ticker.split(':')[1];
        paneEl.querySelector('.pane-title').textContent = symbol;
    }
    
    closeTickCountModal();
}

// Open tick size multiplier modal for a specific pane
function openTickSizeModal(paneIndex) {
    state.activeTickSizePane = paneIndex;
    const pane = state.panes[paneIndex];
    const currentMultiplier = pane?.footprintState?.tickSizeMultiplier || 50;
    const baseTickSize = pane?.footprintState?.baseTickSize || 0.1;
    
    // Update modal to show current selection
    document.querySelectorAll('.tick-size-preset-btn').forEach(btn => {
        btn.classList.toggle('active', parseInt(btn.dataset.multiplier) === currentMultiplier);
    });
    document.getElementById('customTickSize').value = '';
    document.getElementById('baseTickSize').textContent = baseTickSize;
    
    document.getElementById('tickSizeModal').classList.add('active');
}

// Close tick size modal
function closeTickSizeModal() {
    state.activeTickSizePane = null;
    document.getElementById('tickSizeModal').classList.remove('active');
}

// Set tick size multiplier for the active pane
function setTickSizeMultiplier(multiplier) {
    const paneIndex = state.activeTickSizePane;
    if (paneIndex === null) return;
    
    const pane = state.panes[paneIndex];
    if (pane && pane.footprintState) {
        // Update tick size multiplier
        pane.footprintState.tickSizeMultiplier = multiplier;
        
        // Recalculate effective tick size
        const baseTickSize = pane.footprintState.baseTickSize || 0.1;
        pane.footprintState.tickSize = baseTickSize * multiplier;
        
        // Re-aggregate ALL existing trades with new tick size
        if (pane.footprintState.reAggregateAllTrades) {
            pane.footprintState.reAggregateAllTrades();
        }
        
        // Update button text
        const tickSizeBtn = document.querySelector(`.tick-size-btn[data-pane="${paneIndex}"]`);
        if (tickSizeBtn) tickSizeBtn.textContent = `${multiplier}x`;
    }
    
    closeTickSizeModal();
}

// Update exchange filter button states
function updateExchangeFilterButtons() {
    document.querySelectorAll('.exchange-btn').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.exchange === state.filters.exchange);
    });
}

// Toggle ticker on/off
function toggleTicker(tickerKey) {
    const existingIndex = state.panes.findIndex(p => p?.ticker === tickerKey);
    
    if (existingIndex >= 0) {
        // Turn off - clear the pane
        clearPane(existingIndex);
    } else {
        // Turn on - find first empty pane
        const emptyIndex = state.panes.findIndex(p => p === null);
        if (emptyIndex >= 0) {
            assignTickerToPane(tickerKey, emptyIndex);
        } else {
            console.warn('All panes are occupied');
        }
    }
    
    renderTickerList();
    saveConfig();
}

// Assign ticker to a specific pane with FOOTPRINT CHART (matching desktop exactly)
function assignTickerToPane(tickerKey, paneIndex) {
    const [exchange, symbol] = tickerKey.split(':');
    
    // Create footprint chart canvas (custom implementation matching desktop)
    const container = document.getElementById(`chart-${paneIndex}`);
    container.innerHTML = ''; // Clear any existing content
    
    const canvas = document.createElement('canvas');
    canvas.width = container.clientWidth;
    canvas.height = container.clientHeight;
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    container.appendChild(canvas);
    
    const ctx = canvas.getContext('2d');
    
    // Footprint chart state
    const footprintState = {
        bars: [], // Array of footprint bars
        currentBar: null,
        tickBuffer: [],
        allTrades: [], // Store ALL raw trades for re-aggregation
        tickCount: 1000, // 1000T aggregation (matching desktop default)
        baseTickSize: 0.1, // Base tick size (auto-detected from price)
        tickSizeMultiplier: 50, // Default 50x multiplier
        tickSize: 5, // Effective tick size = baseTickSize * multiplier
        lastPrice: null,
        highPrice: null,
        lowPrice: null,
        viewOffset: 0,
        scale: 1,
        priceRange: { high: 0, low: 0 },
        mousePos: null,
        renderFootprint: null, // Will hold reference to render function
        createFootprintBar: null, // Will hold reference to bar creation function
    };
    
    // Colors matching desktop footprint chart exactly
    // Desktop uses: buy_qty on RIGHT (green), sell_qty on LEFT (red)
    // Bar alpha is 0.25 when showing text
    const colors = {
        background: '#131313',
        gridLine: '#1e1e1e',
        textColor: '#c0c0c0',
        // LEFT side = sells (red)
        sellBackground: 'rgba(239, 83, 80, 0.25)',
        sellText: '#ef5350',
        // RIGHT side = buys (green)  
        buyBackground: 'rgba(38, 166, 154, 0.25)',
        buyText: '#26a69a',
        // Wick colors (weak/muted versions with 0.6 alpha)
        wickBullish: 'rgba(38, 166, 154, 0.6)',
        wickBearish: 'rgba(239, 83, 80, 0.6)',
        // Body colors (weak versions)
        bodyBullish: 'rgba(38, 166, 154, 0.5)',
        bodyBearish: 'rgba(239, 83, 80, 0.5)',
        pocHighlight: 'rgba(255, 193, 7, 0.15)',
        currentPriceLine: '#26a69a',
        crosshair: '#505050',
    };
    
    // Create footprint bar from tick buffer
    function createFootprintBar(ticks) {
        if (ticks.length === 0) return null;
        
        const prices = ticks.map(t => t.price);
        const open = ticks[0].price;
        const close = ticks[ticks.length - 1].price;
        const high = Math.max(...prices);
        const low = Math.min(...prices);
        
        // Auto-detect base tick size from price if not set
        if (!footprintState.baseTickSize || footprintState.baseTickSize === 0.1) {
            // Determine base tick size based on price magnitude
            if (close > 10000) footprintState.baseTickSize = 0.1;
            else if (close > 1000) footprintState.baseTickSize = 0.01;
            else if (close > 100) footprintState.baseTickSize = 0.001;
            else footprintState.baseTickSize = 0.0001;
        }
        
        // Calculate effective tick size = base * multiplier
        const tickSize = footprintState.baseTickSize * footprintState.tickSizeMultiplier;
        footprintState.tickSize = tickSize;
        
        // Group trades by price level (footprint cells)
        const priceLevels = {};
        const roundToTick = (p) => Math.round(p / tickSize) * tickSize;
        
        ticks.forEach(trade => {
            const level = roundToTick(trade.price);
            if (!priceLevels[level]) {
                priceLevels[level] = { bid: 0, ask: 0, delta: 0 };
            }
            // is_buyer_maker: true = sell (taker sold), false = buy (taker bought)
            if (trade.is_buyer_maker) {
                priceLevels[level].bid += trade.quantity || 1;
            } else {
                priceLevels[level].ask += trade.quantity || 1;
            }
            priceLevels[level].delta = priceLevels[level].ask - priceLevels[level].bid;
        });
        
        // Find POC (Point of Control) - price level with highest volume
        let pocPrice = null;
        let maxVolume = 0;
        Object.entries(priceLevels).forEach(([price, data]) => {
            const totalVol = data.bid + data.ask;
            if (totalVol > maxVolume) {
                maxVolume = totalVol;
                pocPrice = parseFloat(price);
            }
        });
        
        return {
            time: Date.now(),
            open, high, low, close,
            priceLevels,
            pocPrice,
            isBullish: close >= open,
        };
    }
    
    // Re-aggregate all trades with current settings
    function reAggregateAllTrades() {
        if (footprintState.allTrades.length === 0) return;
        
        // Clear existing bars
        footprintState.bars = [];
        footprintState.currentBar = null;
        footprintState.tickBuffer = [];
        
        // Re-aggregate all trades with current tickCount and tickSize settings
        const tickCount = footprintState.tickCount;
        let buffer = [];
        
        footprintState.allTrades.forEach((trade, index) => {
            buffer.push(trade);
            
            if (buffer.length >= tickCount) {
                const bar = createFootprintBar(buffer);
                if (bar) footprintState.bars.push(bar);
                buffer = [];
            }
        });
        
        // Keep remaining trades in buffer as current forming bar
        footprintState.tickBuffer = buffer;
        if (buffer.length > 0) {
            footprintState.currentBar = createFootprintBar(buffer);
        }
        
        // Limit bars for performance
        if (footprintState.bars.length > 50) {
            footprintState.bars = footprintState.bars.slice(-40);
        }
        
        renderFootprint();
    }
    
    // Store function references for external access
    footprintState.reAggregateAllTrades = reAggregateAllTrades;
    footprintState.createFootprintBar = createFootprintBar;
    
    // Render footprint chart (matching desktop exactly)
    function renderFootprint() {
        const width = canvas.width;
        const height = canvas.height;
        const priceAxisWidth = 70;
        const timeAxisHeight = 25;
        const chartWidth = width - priceAxisWidth;
        const chartHeight = height - timeAxisHeight;
        
        // Clear canvas
        ctx.fillStyle = colors.background;
        ctx.fillRect(0, 0, width, height);
        
        // Calculate price range from all bars
        let allHigh = footprintState.highPrice || 70000;
        let allLow = footprintState.lowPrice || 60000;
        
        if (footprintState.bars.length > 0) {
            allHigh = Math.max(...footprintState.bars.map(b => b.high));
            allLow = Math.min(...footprintState.bars.map(b => b.low));
        }
        if (footprintState.currentBar) {
            allHigh = Math.max(allHigh, footprintState.currentBar.high);
            allLow = Math.min(allLow, footprintState.currentBar.low);
        }
        
        // Add padding to price range
        let priceRange = allHigh - allLow;
        const padding = priceRange * 0.1 || 50;
        allHigh += padding;
        allLow -= padding;
        
        // Apply zoom scale to price range
        const zoomScale = footprintState.zoomScale || 1;
        const midPrice = (allHigh + allLow) / 2;
        const zoomedRange = (allHigh - allLow) * zoomScale;
        allHigh = midPrice + zoomedRange / 2;
        allLow = midPrice - zoomedRange / 2;
        
        // Apply vertical pan offset
        const priceOffset = footprintState.priceOffset || 0;
        allHigh += priceOffset;
        allLow += priceOffset;
        
        footprintState.priceRange = { high: allHigh, low: allLow };
        
        const tickSize = footprintState.tickSize || 5;
        const priceToY = (price) => {
            return chartHeight - ((price - allLow) / (allHigh - allLow)) * chartHeight;
        };
        const yToPrice = (y) => {
            return allLow + ((chartHeight - y) / chartHeight) * (allHigh - allLow);
        };
        
        // Draw horizontal grid lines at price levels
        ctx.strokeStyle = colors.gridLine;
        ctx.lineWidth = 1;
        const startPrice = Math.ceil(allLow / tickSize) * tickSize;
        for (let price = startPrice; price <= allHigh; price += tickSize * 2) {
            const y = priceToY(price);
            if (y >= 0 && y <= chartHeight) {
                ctx.beginPath();
                ctx.moveTo(0, y);
                ctx.lineTo(chartWidth, y);
                ctx.stroke();
            }
        }
        
        // Draw footprint bars
        const allBars = [...footprintState.bars];
        if (footprintState.currentBar) {
            allBars.push(footprintState.currentBar);
        }
        
        // Apply zoom scale to bar width
        const baseBarWidth = 80;
        const barWidthScale = footprintState.barWidthScale || 1;
        const barWidth = baseBarWidth * barWidthScale;
        const cellPadding = 2;
        const visibleBars = Math.floor(chartWidth / barWidth);
        
        // Apply view offset for horizontal scrolling
        const viewOffset = footprintState.viewOffset || 0;
        const endIdx = Math.max(0, allBars.length - viewOffset);
        const startIdx = Math.max(0, endIdx - visibleBars);
        
        allBars.slice(startIdx, endIdx).forEach((bar, idx) => {
            const x = idx * barWidth + 10;
            const barCenterX = x + barWidth / 2;
            const highY = priceToY(bar.high);
            const lowY = priceToY(bar.low);
            const openY = priceToY(bar.open);
            const closeY = priceToY(bar.close);
            
            // Calculate cell dimensions
            const cellWidth = (barWidth - 16) / 2; // Space for cells on each side
            const candleWidth = 4; // Thin candle body width
            
            // Draw footprint cells at each price level FIRST (behind candle)
            Object.entries(bar.priceLevels).forEach(([priceStr, data]) => {
                const price = parseFloat(priceStr);
                const y = priceToY(price);
                const cellHeight = Math.abs(priceToY(price) - priceToY(price + tickSize));
                
                if (y < 0 || y > chartHeight) return;
                
                // LEFT side = sells (red) - data.bid is sell volume
                const sellX = barCenterX - cellWidth - candleWidth/2 - 2;
                // RIGHT side = buys (green) - data.ask is buy volume  
                const buyX = barCenterX + candleWidth/2 + 2;
                
                // POC highlight (full width background)
                if (price === bar.pocPrice) {
                    ctx.fillStyle = colors.pocHighlight;
                    ctx.fillRect(sellX, y - cellHeight/2, cellWidth * 2 + candleWidth + 8, cellHeight);
                }
                
                // Draw SELL volume - LEFT side with red background
                if (data.bid > 0) {
                    ctx.fillStyle = colors.sellBackground;
                    ctx.fillRect(sellX, y - cellHeight/2 + 1, cellWidth, cellHeight - 2);
                    
                    ctx.fillStyle = colors.sellText;
                    ctx.font = '10px monospace';
                    ctx.textAlign = 'right';
                    ctx.textBaseline = 'middle';
                    ctx.fillText(formatVolume(data.bid), sellX + cellWidth - 2, y);
                }
                
                // Draw BUY volume - RIGHT side with green background
                if (data.ask > 0) {
                    ctx.fillStyle = colors.buyBackground;
                    ctx.fillRect(buyX, y - cellHeight/2 + 1, cellWidth, cellHeight - 2);
                    
                    ctx.fillStyle = colors.buyText;
                    ctx.font = '10px monospace';
                    ctx.textAlign = 'left';
                    ctx.textBaseline = 'middle';
                    ctx.fillText(formatVolume(data.ask), buyX + 2, y);
                }
            });
            
            // Draw candle wick (thin line from high to low) - 1px width, 0.6 alpha
            ctx.strokeStyle = bar.isBullish ? colors.wickBullish : colors.wickBearish;
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(barCenterX, highY);
            ctx.lineTo(barCenterX, lowY);
            ctx.stroke();
            
            // Draw candle body (small rectangle at open/close)
            const bodyTop = Math.min(openY, closeY);
            const bodyHeight = Math.abs(openY - closeY) || 2; // Min 2px height
            ctx.fillStyle = bar.isBullish ? colors.bodyBullish : colors.bodyBearish;
            ctx.fillRect(barCenterX - candleWidth/2, bodyTop, candleWidth, bodyHeight);
        });
        
        // Draw price axis (right side)
        ctx.fillStyle = '#1a1a1a';
        ctx.fillRect(chartWidth, 0, priceAxisWidth, chartHeight);
        
        ctx.strokeStyle = colors.gridLine;
        ctx.beginPath();
        ctx.moveTo(chartWidth, 0);
        ctx.lineTo(chartWidth, chartHeight);
        ctx.stroke();
        
        // Price labels
        ctx.fillStyle = colors.textColor;
        ctx.font = '11px -apple-system, sans-serif';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        
        for (let price = startPrice; price <= allHigh; price += tickSize * 4) {
            const y = priceToY(price);
            if (y >= 10 && y <= chartHeight - 10) {
                ctx.fillText(price.toFixed(0), chartWidth + 5, y);
            }
        }
        
        // Current price highlight (cyan dashed line matching desktop)
        if (footprintState.lastPrice) {
            const currentY = priceToY(footprintState.lastPrice);
            const labelHeight = 18;
            const isBullish = footprintState.currentBar?.isBullish ?? true;
            
            // Draw price line across chart (cyan/teal dashed line like desktop)
            ctx.strokeStyle = colors.currentPriceLine;
            ctx.setLineDash([4, 4]);
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(0, currentY);
            ctx.lineTo(chartWidth, currentY);
            ctx.stroke();
            ctx.setLineDash([]);
            
            // Draw price label background
            ctx.fillStyle = isBullish ? colors.buyText : colors.sellText;
            ctx.fillRect(chartWidth, currentY - labelHeight/2, priceAxisWidth, labelHeight);
            
            // Draw price text
            ctx.fillStyle = '#ffffff';
            ctx.font = 'bold 11px monospace';
            ctx.textAlign = 'left';
            ctx.textBaseline = 'middle';
            ctx.fillText(footprintState.lastPrice.toFixed(0), chartWidth + 5, currentY);
        }
        
        // Draw time axis (bottom)
        ctx.fillStyle = '#1a1a1a';
        ctx.fillRect(0, chartHeight, width, timeAxisHeight);
        
        ctx.strokeStyle = colors.gridLine;
        ctx.beginPath();
        ctx.moveTo(0, chartHeight);
        ctx.lineTo(width, chartHeight);
        ctx.stroke();
        
        // Time labels
        ctx.fillStyle = colors.textColor;
        ctx.font = '11px -apple-system, sans-serif';
        ctx.textAlign = 'center';
        
        allBars.slice(startIdx, endIdx).forEach((bar, idx) => {
            const x = idx * barWidth + barWidth / 2 + 10;
            const time = new Date(bar.time);
            const timeStr = `${time.getHours().toString().padStart(2, '0')}:${time.getMinutes().toString().padStart(2, '0')}`;
            ctx.fillText(timeStr, x, chartHeight + 15);
        });
        
        // Draw crosshair if mouse is over chart
        if (footprintState.mousePos) {
            const { x, y } = footprintState.mousePos;
            if (x < chartWidth && y < chartHeight) {
                ctx.strokeStyle = colors.crosshair;
                ctx.setLineDash([4, 4]);
                ctx.lineWidth = 1;
                
                // Vertical line
                ctx.beginPath();
                ctx.moveTo(x, 0);
                ctx.lineTo(x, chartHeight);
                ctx.stroke();
                
                // Horizontal line
                ctx.beginPath();
                ctx.moveTo(0, y);
                ctx.lineTo(chartWidth, y);
                ctx.stroke();
                
                ctx.setLineDash([]);
                
                // Price label at crosshair
                const crosshairPrice = yToPrice(y);
                ctx.fillStyle = '#2a2a2a';
                ctx.fillRect(chartWidth, y - 9, priceAxisWidth, 18);
                ctx.fillStyle = colors.textColor;
                ctx.textAlign = 'left';
                ctx.fillText(crosshairPrice.toFixed(0), chartWidth + 5, y);
            }
        }
    }
    
    // Format volume for display
    function formatVolume(vol) {
        if (vol >= 1000) return (vol / 1000).toFixed(1) + 'k';
        if (vol >= 100) return vol.toFixed(0);
        if (vol >= 10) return vol.toFixed(1);
        return vol.toFixed(2);
    }
    
    // Navigation state
    let isDragging = false;
    let dragStartX = 0;
    let dragStartY = 0;
    let dragStartOffsetX = 0;
    let dragStartOffsetY = 0;
    
    // Mouse event handlers for crosshair and navigation
    canvas.addEventListener('mousemove', (e) => {
        const rect = canvas.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;
        
        footprintState.mousePos = { x, y };
        
        // Handle panning (drag)
        if (isDragging) {
            const deltaX = x - dragStartX;
            const deltaY = y - dragStartY;
            
            // Horizontal pan (scroll through bars)
            footprintState.viewOffset = dragStartOffsetX - Math.round(deltaX / 40);
            footprintState.viewOffset = Math.max(0, footprintState.viewOffset);
            
            // Vertical pan (price range shift)
            const priceRange = footprintState.priceRange.high - footprintState.priceRange.low;
            const priceDelta = (deltaY / canvas.height) * priceRange;
            footprintState.priceOffset = dragStartOffsetY + priceDelta;
        }
        
        renderFootprint();
    });
    
    canvas.addEventListener('mouseleave', () => {
        footprintState.mousePos = null;
        isDragging = false;
        renderFootprint();
    });
    
    // Mouse down - start drag
    canvas.addEventListener('mousedown', (e) => {
        if (e.button === 0) { // Left click
            isDragging = true;
            const rect = canvas.getBoundingClientRect();
            dragStartX = e.clientX - rect.left;
            dragStartY = e.clientY - rect.top;
            dragStartOffsetX = footprintState.viewOffset || 0;
            dragStartOffsetY = footprintState.priceOffset || 0;
            canvas.style.cursor = 'grabbing';
        }
    });
    
    // Mouse up - end drag
    canvas.addEventListener('mouseup', () => {
        isDragging = false;
        canvas.style.cursor = 'crosshair';
    });
    
    // Mouse wheel - zoom
    canvas.addEventListener('wheel', (e) => {
        e.preventDefault();
        const rect = canvas.getBoundingClientRect();
        const mouseY = e.clientY - rect.top;
        const chartHeight = canvas.height - 25; // minus time axis
        
        // Zoom factor
        const zoomFactor = e.deltaY > 0 ? 1.1 : 0.9;
        
        // Adjust price scale (vertical zoom)
        if (e.shiftKey) {
            // Horizontal zoom - adjust bar width
            footprintState.barWidthScale = (footprintState.barWidthScale || 1) * (e.deltaY > 0 ? 0.9 : 1.1);
            footprintState.barWidthScale = Math.max(0.5, Math.min(2, footprintState.barWidthScale));
        } else {
            // Vertical zoom - adjust price range
            const currentRange = footprintState.priceRange.high - footprintState.priceRange.low;
            const newRange = currentRange * zoomFactor;
            
            // Zoom centered on mouse position
            const mousePrice = footprintState.priceRange.low + 
                ((chartHeight - mouseY) / chartHeight) * currentRange;
            
            const mouseRatio = (mousePrice - footprintState.priceRange.low) / currentRange;
            
            footprintState.zoomScale = (footprintState.zoomScale || 1) * zoomFactor;
            footprintState.zoomScale = Math.max(0.2, Math.min(5, footprintState.zoomScale));
        }
        
        // Update zoom level display
        updateZoomLevel(paneIndex, footprintState.zoomScale || 1);
        
        renderFootprint();
    }, { passive: false });
    
    // Update zoom level display function
    function updateZoomLevel(paneIdx, scale) {
        const zoomEl = document.querySelector(`.chart-pane[data-pane="${paneIdx}"] .zoom-level`);
        if (zoomEl) {
            const zoomPercent = Math.round((1 / scale) * 50);
            zoomEl.textContent = `${zoomPercent}x`;
        }
    }
    
    // Set default cursor
    canvas.style.cursor = 'crosshair';
    
    // Handle resize
    const resizeObserver = new ResizeObserver(() => {
        canvas.width = container.clientWidth;
        canvas.height = container.clientHeight;
        renderFootprint();
    });
    resizeObserver.observe(container);
    
    // Initial render
    renderFootprint();
    
    // Connect WebSocket with auto-reconnect
    function connectWebSocket() {
        const ws = new WebSocket(`${WS_BASE}/ws/live/${exchange}/${symbol}`);
        
        ws.onopen = () => {
            console.log(`WebSocket connected: ${tickerKey}`);
            updateConnectionStatus('connected');
        };
        
        ws.onmessage = (event) => {
            try {
                const trade = JSON.parse(event.data);
                
                // Store ALL trades for re-aggregation when settings change
                footprintState.allTrades.push(trade);
                
                // Limit stored trades for memory (keep last 100k trades)
                if (footprintState.allTrades.length > 100000) {
                    footprintState.allTrades = footprintState.allTrades.slice(-80000);
                }
                
                // Add to tick buffer
                footprintState.tickBuffer.push(trade);
                footprintState.lastPrice = trade.price;
                
                // Update price range
                if (!footprintState.highPrice || trade.price > footprintState.highPrice) {
                    footprintState.highPrice = trade.price;
                }
                if (!footprintState.lowPrice || trade.price < footprintState.lowPrice) {
                    footprintState.lowPrice = trade.price;
                }
                
                // Update ticker price in state
                const ticker = state.tickers.find(t => `${t.exchange}:${t.symbol}` === tickerKey);
                if (ticker) {
                    ticker.price = trade.price;
                }
                
                // Create/update current forming bar
                footprintState.currentBar = createFootprintBar(footprintState.tickBuffer);
                
                // Complete bar when tick count reached
                if (footprintState.tickBuffer.length >= footprintState.tickCount) {
                    footprintState.bars.push(footprintState.currentBar);
                    footprintState.tickBuffer = [];
                    footprintState.currentBar = null;
                    
                    // Keep max 50 bars for performance
                    if (footprintState.bars.length > 50) {
                        footprintState.bars = footprintState.bars.slice(-40);
                    }
                }
                
                // Render updated chart
                renderFootprint();
                
                // Update pane title with price
                updatePaneTitle(paneIndex, symbol, trade.price);
                
            } catch (e) {
                console.error('Failed to parse trade:', e);
            }
        };
        
        ws.onerror = (e) => {
            console.error(`WebSocket error: ${tickerKey}`, e);
            updateConnectionStatus('disconnected');
        };
        
        ws.onclose = () => {
            console.log(`WebSocket closed: ${tickerKey}`);
            
            // Auto-reconnect if pane still exists
            const pane = state.panes[paneIndex];
            if (pane && pane.ticker === tickerKey) {
                updateConnectionStatus('reconnecting');
                pane.reconnectTimer = setTimeout(() => {
                    console.log(`Reconnecting to ${tickerKey}...`);
                    pane.ws = connectWebSocket();
                }, RECONNECT_DELAY);
            }
        };
        
        return ws;
    }
    
    const ws = connectWebSocket();
    
    // Store pane state
    state.panes[paneIndex] = {
        ticker: tickerKey,
        canvas,
        footprintState,
        resizeObserver,
        ws,
        reconnectTimer: null,
    };
    
    // Update UI
    const paneEl = document.querySelector(`.chart-pane[data-pane="${paneIndex}"]`);
    paneEl.classList.add('has-chart');
    paneEl.querySelector('.pane-title').textContent = `${symbol} 1000T`;
}

// Aggregate ticks into a single candle
function aggregateTicksToCandle(ticks, index) {
    if (ticks.length === 0) return null;
    
    const prices = ticks.map(t => t.price);
    // Use sequential time starting from a base timestamp
    // Each candle is 1 second apart for proper chart display
    const baseTime = Math.floor(Date.now() / 1000) - 1000 + index;
    
    return {
        time: baseTime,
        open: ticks[0].price,
        high: Math.max(...prices),
        low: Math.min(...prices),
        close: ticks[ticks.length - 1].price,
    };
}

// Update pane title with current price
function updatePaneTitle(paneIndex, symbol, price) {
    const paneEl = document.querySelector(`.chart-pane[data-pane="${paneIndex}"]`);
    const formattedPrice = price.toLocaleString('en-US', {
        minimumFractionDigits: 2,
        maximumFractionDigits: price < 1 ? 6 : 2
    });
    paneEl.querySelector('.pane-title').textContent = `${symbol} $${formattedPrice}`;
}

// Clear a pane
function clearPane(paneIndex) {
    const pane = state.panes[paneIndex];
    if (!pane) return;
    
    // Clear reconnect timer
    if (pane.reconnectTimer) {
        clearTimeout(pane.reconnectTimer);
    }
    
    // Close WebSocket
    if (pane.ws) {
        pane.ws.close();
    }
    
    // Remove resize observer (for footprint chart)
    if (pane.resizeObserver) {
        pane.resizeObserver.disconnect();
    }
    
    // Remove chart (Lightweight Charts) or canvas (footprint)
    if (pane.chart) {
        pane.chart.remove();
    }
    
    // Clear canvas container
    const container = document.getElementById(`chart-${paneIndex}`);
    if (container) {
        container.innerHTML = '';
    }
    
    // Clear state
    state.panes[paneIndex] = null;
    
    // Update UI
    const paneEl = document.querySelector(`.chart-pane[data-pane="${paneIndex}"]`);
    paneEl.classList.remove('has-chart', 'maximized');
    paneEl.querySelector('.pane-title').textContent = '';
    
    renderTickerList();
    saveConfig();
}

// Save configuration to server
async function saveConfig() {
    const config = {
        pane_tickers: state.panes.map(p => p?.ticker || null),
        filters: {
            large_volume_only: state.filters.largeVolumeOnly,
            market_type: state.filters.marketType,
            selected_exchanges: [],
        }
    };
    
    try {
        await fetch(`${API_BASE}/api/v1/config`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(config),
        });
    } catch (e) {
        console.error('Failed to save config:', e);
    }
    
    // Also save to localStorage as backup
    localStorage.setItem('tickCollectorConfig', JSON.stringify(config));
}

// Restore panes from saved config
function restorePanes() {
    // Try server config first
    if (state.config?.pane_tickers) {
        state.config.pane_tickers.forEach((ticker, i) => {
            if (ticker) {
                assignTickerToPane(ticker, i);
            }
        });
        return;
    }
    
    // Fall back to localStorage
    try {
        const saved = localStorage.getItem('tickCollectorConfig');
        if (saved) {
            const config = JSON.parse(saved);
            config.pane_tickers?.forEach((ticker, i) => {
                if (ticker) {
                    assignTickerToPane(ticker, i);
                }
            });
        }
    } catch (e) {
        console.error('Failed to restore panes:', e);
    }
}
