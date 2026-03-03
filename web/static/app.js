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
const DATA_RETENTION_DAYS = 7; // Keep 7 days of data
const SAVE_INTERVAL_MS = 10000; // Auto-save every 10 seconds (continuous collection)
const SAVE_ON_BAR_COUNT = 5; // Also save every N completed bars
const STORAGE_KEY_PREFIX = 'tc_footprint_'; // localStorage key prefix for footprint data

// Initialize app
document.addEventListener('DOMContentLoaded', async () => {
    console.log('🚀 Tick Collector Web starting...');
    
    // Clean up old data (7-day retention)
    cleanupOldData();
    
    // Load favorites from localStorage
    loadFavorites();
    
    // Load config
    await loadConfig();
    
    // Load tickers
    await loadTickers();
    
    // Setup event listeners
    setupEventListeners();
    
    // Restore saved pane assignments (async - loads data from server)
    await restorePanes();
    
    // Update ticker count
    updateTickerCount();
    
    // Start periodic auto-save
    startAutoSave();
    
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

// ============================================================================
// LOCAL STORAGE PERSISTENCE FOR FOOTPRINT DATA (7-DAY RETENTION)
// ============================================================================

// Save footprint data for a specific ticker to SERVER (file-based persistence)
async function saveFootprintData(tickerKey, footprintState) {
    try {
        const [exchange, symbol] = tickerKey.split(':');
        
        // Prepare data for storage - only save essential data
        const dataToSave = {
            timestamp: Date.now(),
            ticker_key: tickerKey,
            settings: {
                tick_count: footprintState.tickCount,
                tick_size_multiplier: footprintState.tickSizeMultiplier,
                base_tick_size: footprintState.baseTickSize,
                tick_size: footprintState.tickSize,
            },
            // Save completed bars (limit to last 200 bars for storage efficiency)
            bars: (footprintState.bars || []).slice(-200).map(bar => ({
                time: bar.time,
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                priceLevels: bar.priceLevels,
                pocPrice: bar.pocPrice,
                isBullish: bar.isBullish,
            })),
            // Save raw trades for re-aggregation (limit to last 50k for storage)
            all_trades: (footprintState.allTrades || []).slice(-50000),
            // Save current buffer
            tick_buffer: footprintState.tickBuffer || [],
            // Price range
            last_price: footprintState.lastPrice,
            high_price: footprintState.highPrice,
            low_price: footprintState.lowPrice,
        };
        
        // Save to server via API
        const response = await fetch(`${API_BASE}/api/v1/footprint/${exchange}/${symbol}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(dataToSave),
        });
        
        if (response.ok) {
            console.log(`💾 Saved ${tickerKey}: ${dataToSave.bars.length} bars, ${dataToSave.all_trades.length} trades (server)`);
        } else {
            console.error(`Failed to save footprint data for ${tickerKey}: ${response.status}`);
        }
    } catch (e) {
        console.error(`Failed to save footprint data for ${tickerKey}:`, e);
    }
}

// Load footprint data for a specific ticker from SERVER (file-based persistence)
async function loadFootprintData(tickerKey) {
    try {
        const [exchange, symbol] = tickerKey.split(':');
        
        const response = await fetch(`${API_BASE}/api/v1/footprint/${exchange}/${symbol}`);
        
        if (!response.ok) {
            console.log(`No saved data for ${tickerKey}`);
            return null;
        }
        
        const data = await response.json();
        
        if (!data) {
            console.log(`No saved data for ${tickerKey}`);
            return null;
        }
        
        // Convert snake_case to camelCase for client use
        const converted = {
            timestamp: data.timestamp,
            tickerKey: data.ticker_key,
            settings: {
                tickCount: data.settings?.tick_count,
                tickSizeMultiplier: data.settings?.tick_size_multiplier,
                baseTickSize: data.settings?.base_tick_size,
                tickSize: data.settings?.tick_size,
            },
            bars: data.bars || [],
            allTrades: data.all_trades || [],
            tickBuffer: data.tick_buffer || [],
            lastPrice: data.last_price,
            highPrice: data.high_price,
            lowPrice: data.low_price,
        };
        
        const ageMs = Date.now() - converted.timestamp;
        const ageDays = ageMs / (1000 * 60 * 60 * 24);
        
        console.log(`📂 Loaded ${tickerKey}: ${converted.bars?.length || 0} bars, ${converted.allTrades?.length || 0} trades (${ageDays.toFixed(1)} days old) [server]`);
        return converted;
    } catch (e) {
        console.error(`Failed to load footprint data for ${tickerKey}:`, e);
        return null;
    }
}

// Clean up old data beyond retention period
function cleanupOldData(aggressive = false) {
    console.log('🧹 Cleaning up old footprint data...');
    const now = Date.now();
    const retentionMs = DATA_RETENTION_DAYS * 24 * 60 * 60 * 1000;
    let removedCount = 0;
    
    // Iterate through all localStorage keys
    for (let i = localStorage.length - 1; i >= 0; i--) {
        const key = localStorage.key(i);
        if (!key || !key.startsWith(STORAGE_KEY_PREFIX)) continue;
        
        try {
            const data = JSON.parse(localStorage.getItem(key));
            const age = now - (data.timestamp || 0);
            
            // Remove if older than retention period, or if aggressive cleanup
            if (age > retentionMs || (aggressive && age > retentionMs / 2)) {
                localStorage.removeItem(key);
                removedCount++;
                console.log(`🗑️ Removed old data: ${key}`);
            }
        } catch (e) {
            // If we can't parse it, remove it
            localStorage.removeItem(key);
            removedCount++;
        }
    }
    
    if (removedCount > 0) {
        console.log(`🧹 Cleaned up ${removedCount} old data entries`);
    }
}

// Start periodic auto-save for all active panes (CONTINUOUS COLLECTION MODE)
function startAutoSave() {
    // Auto-save every SAVE_INTERVAL_MS (10 seconds)
    setInterval(() => {
        state.panes.forEach((pane, index) => {
            if (pane && pane.ticker && pane.footprintState) {
                saveFootprintData(pane.ticker, pane.footprintState);
            }
        });
    }, SAVE_INTERVAL_MS);
    
    // Periodic cleanup every hour (7-day retention)
    setInterval(() => {
        cleanupOldData();
    }, 60 * 60 * 1000); // Every hour
    
    // Also save on page unload (backup)
    window.addEventListener('beforeunload', () => {
        state.panes.forEach((pane, index) => {
            if (pane && pane.ticker && pane.footprintState) {
                saveFootprintData(pane.ticker, pane.footprintState);
            }
        });
    });
    
    console.log(`⏰ Continuous save started (every ${SAVE_INTERVAL_MS / 1000}s + every ${SAVE_ON_BAR_COUNT} bars)`);
    console.log(`🧹 Retention cleanup scheduled (every hour, ${DATA_RETENTION_DAYS}-day retention)`);
}

// Get storage usage info
function getStorageInfo() {
    let totalSize = 0;
    let footprintSize = 0;
    let footprintCount = 0;
    
    for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i);
        const value = localStorage.getItem(key);
        const size = (key.length + value.length) * 2; // UTF-16 = 2 bytes per char
        totalSize += size;
        
        if (key.startsWith(STORAGE_KEY_PREFIX)) {
            footprintSize += size;
            footprintCount++;
        }
    }
    
    return {
        totalSizeMB: (totalSize / (1024 * 1024)).toFixed(2),
        footprintSizeMB: (footprintSize / (1024 * 1024)).toFixed(2),
        footprintCount,
    };
}

// ============================================================================

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
    
    // Main tab navigation
    document.querySelectorAll('.main-tab').forEach(tab => {
        tab.addEventListener('click', () => {
            const tabName = tab.dataset.tab;
            switchTab(tabName);
        });
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
async function toggleTicker(tickerKey) {
    const existingIndex = state.panes.findIndex(p => p?.ticker === tickerKey);
    
    if (existingIndex >= 0) {
        // Turn off - clear the pane
        clearPane(existingIndex);
    } else {
        // Turn on - find first empty pane
        const emptyIndex = state.panes.findIndex(p => p === null);
        if (emptyIndex >= 0) {
            await assignTickerToPane(tickerKey, emptyIndex);
        } else {
            console.warn('All panes are occupied');
        }
    }
    
    renderTickerList();
    saveConfig();
}

// Assign ticker to a specific pane with FOOTPRINT CHART (matching desktop exactly)
async function assignTickerToPane(tickerKey, paneIndex) {
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
    
    // Try to load saved data from SERVER (async)
    const savedData = await loadFootprintData(tickerKey);
    
    // Footprint chart state - restore from saved data if available
    const footprintState = {
        bars: savedData?.bars || [], // Array of footprint bars
        currentBar: null,
        tickBuffer: savedData?.tickBuffer || [],
        allTrades: savedData?.allTrades || [], // Store ALL raw trades for re-aggregation
        tickCount: savedData?.settings?.tickCount || 1000, // 1000T aggregation (matching desktop default)
        baseTickSize: savedData?.settings?.baseTickSize || 0.1, // Base tick size (auto-detected from price)
        tickSizeMultiplier: savedData?.settings?.tickSizeMultiplier || 50, // Default 50x multiplier
        tickSize: savedData?.settings?.tickSize || 5, // Effective tick size = baseTickSize * multiplier
        lastPrice: savedData?.lastPrice || null,
        highPrice: savedData?.highPrice || null,
        lowPrice: savedData?.lowPrice || null,
        viewOffset: 0,
        scale: 1,
        priceRange: { high: 0, low: 0 },
        mousePos: null,
        renderFootprint: null, // Will hold reference to render function
        createFootprintBar: null, // Will hold reference to bar creation function
    };
    
    // Update UI buttons with restored settings
    const tickBtn = document.querySelector(`.tick-count-btn[data-pane="${paneIndex}"]`);
    if (tickBtn) tickBtn.textContent = `${footprintState.tickCount}T`;
    const tickSizeBtn = document.querySelector(`.tick-size-btn[data-pane="${paneIndex}"]`);
    if (tickSizeBtn) tickSizeBtn.textContent = `${footprintState.tickSizeMultiplier}x`;
    
    if (savedData) {
        console.log(`📊 Restored ${tickerKey}: ${footprintState.bars.length} bars, ${footprintState.allTrades.length} trades`);
    }
    
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
    
    // Store function references for external access (backup data re-rendering)
    footprintState.renderFootprint = renderFootprint;
    footprintState.reAggregateAllTrades = reAggregateAllTrades;
    footprintState.createFootprintBar = createFootprintBar;
    
    // Initial render
    renderFootprint();
    
    // Connect WebSocket with auto-reconnect
    function connectWebSocket() {
        const ws = new WebSocket(`${WS_BASE}/ws/live/${exchange}/${symbol}`);
        
        ws.onopen = () => {
            console.log(`WebSocket connected: ${tickerKey}`);
            updateConnectionStatus('connected');
            
            // BACKUP: Fill gap between last saved data and now
            // This handles the case where app was offline and is now reconnecting
            fillDataGap(exchange, symbol, footprintState);
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
                    footprintState.barsSinceLastSave = (footprintState.barsSinceLastSave || 0) + 1;
                    
                    // Keep max 200 bars for display (more for storage)
                    if (footprintState.bars.length > 200) {
                        footprintState.bars = footprintState.bars.slice(-150);
                    }
                    
                    // Trim allTrades to prevent memory overflow (keep last 100k)
                    if (footprintState.allTrades.length > 100000) {
                        footprintState.allTrades = footprintState.allTrades.slice(-80000);
                    }
                    
                    // CONTINUOUS SAVE: Save every N completed bars
                    if (footprintState.barsSinceLastSave >= SAVE_ON_BAR_COUNT) {
                        saveFootprintData(tickerKey, footprintState);
                        footprintState.barsSinceLastSave = 0;
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
            
            // Record disconnect time for gap calculation on reconnect
            footprintState.lastDisconnectTime = Date.now();
            
            // Auto-reconnect if pane still exists
            const pane = state.panes[paneIndex];
            if (pane && pane.ticker === tickerKey) {
                updateConnectionStatus('reconnecting');
                
                // Note: Backup data will be fetched on reconnect (ws.onopen)
                // This ensures we fill the gap with the most recent data available
                
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
    
    // Save footprint data before clearing (persist to localStorage)
    if (pane.ticker && pane.footprintState) {
        saveFootprintData(pane.ticker, pane.footprintState);
    }
    
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
async function restorePanes() {
    // Try server config first
    if (state.config?.pane_tickers) {
        for (let i = 0; i < state.config.pane_tickers.length; i++) {
            const ticker = state.config.pane_tickers[i];
            if (ticker) {
                await assignTickerToPane(ticker, i);
            }
        }
        return;
    }
    
    // Fall back to localStorage
    try {
        const saved = localStorage.getItem('tickCollectorConfig');
        if (saved) {
            const config = JSON.parse(saved);
            if (config.pane_tickers) {
                for (let i = 0; i < config.pane_tickers.length; i++) {
                    const ticker = config.pane_tickers[i];
                    if (ticker) {
                        await assignTickerToPane(ticker, i);
                    }
                }
            }
        }
    } catch (e) {
        console.error('Failed to restore panes:', e);
    }
}

// ============================================================================
// BACKUP TRADES - Fetch from REST API when WebSocket disconnects
// ============================================================================

// Fill data gap when WebSocket connects - fetches trades to fill time gap
// Called on WebSocket OPEN to fill gap between last saved data and current time
async function fillDataGap(exchange, symbol, footprintState) {
    const tickerKey = `${exchange}:${symbol}`;
    
    console.log(`🔍 Checking for data gap: ${tickerKey}, existing trades: ${footprintState.allTrades.length}`);
    
    // Always try to fetch backup data to fill any gaps
    // Even if we have no existing data, we can get recent trades
    const lastTradeTime = footprintState.allTrades.length > 0 
        ? footprintState.allTrades[footprintState.allTrades.length - 1].timestamp 
        : 0;
    
    const now = Date.now();
    const gapMs = lastTradeTime > 0 ? (now - lastTradeTime) : 0;
    const gapMinutes = gapMs / (1000 * 60);
    
    // Check if gap is too old (> 7 days) - clear and start fresh
    const MAX_GAP_DAYS = 7;
    if (lastTradeTime > 0 && gapMs > MAX_GAP_DAYS * 24 * 60 * 60 * 1000) {
        console.log(`⚠️ Gap too large for ${tickerKey} (${(gapMs / (1000 * 60 * 60 * 24)).toFixed(1)} days), starting fresh`);
        footprintState.allTrades = [];
        footprintState.bars = [];
        footprintState.tickBuffer = [];
        return;
    }
    
    // Fetch backup data if we have a gap > 10 seconds (to avoid unnecessary calls)
    const MIN_GAP_SECONDS = 10;
    if (lastTradeTime === 0 || gapMs > MIN_GAP_SECONDS * 1000) {
        console.log(`🔄 Filling gap for ${tickerKey}: ${gapMinutes.toFixed(1)} minutes since last trade`);
        await fetchBackupTrades(exchange, symbol, footprintState, lastTradeTime);
    } else {
        console.log(`✅ No significant gap for ${tickerKey} (${(gapMs/1000).toFixed(1)}s)`);
    }
}

// Fetch backup trades from exchange REST API to fill gaps
// ONLY called for active tickers (those with WebSocket connections)
async function fetchBackupTrades(exchange, symbol, footprintState, sinceTimestamp = 0) {
    const tickerKey = `${exchange}:${symbol}`;
    
    // Double-check: Only fetch backup for active tickers
    const isActive = state.panes.some(p => p && p.ticker === tickerKey);
    if (!isActive) {
        console.log(`⏭️ Skipping backup fetch for ${tickerKey} - ticker not active`);
        return;
    }
    
    console.log(`🔄 Fetching backup trades for ${tickerKey} (since ${sinceTimestamp ? new Date(sinceTimestamp).toISOString() : 'beginning'})...`);
    
    try {
        const response = await fetch(`${API_BASE}/api/v1/backup-trades/${exchange}/${symbol}?limit=1000`);
        
        if (!response.ok) {
            console.error(`Failed to fetch backup trades: ${response.status}`);
            return;
        }
        
        const trades = await response.json();
        
        if (!trades || trades.length === 0) {
            console.log(`No backup trades available for ${tickerKey}`);
            return;
        }
        
        console.log(`📥 Received ${trades.length} backup trades for ${tickerKey}`);
        console.log(`📊 Backup trades time range: ${new Date(trades[0]?.timestamp).toISOString()} to ${new Date(trades[trades.length-1]?.timestamp).toISOString()}`);
        
        // Get existing trade timestamps for deduplication
        const existingTimestamps = new Set(footprintState.allTrades.map(t => t.timestamp));
        
        // Filter to only include trades that don't already exist (deduplication only)
        // We want ALL trades from the REST API that we don't have, regardless of timestamp
        const newTrades = trades.filter(t => !existingTimestamps.has(t.timestamp));
        
        console.log(`🔍 After deduplication: ${newTrades.length} unique trades to add`);
        
        if (newTrades.length === 0) {
            console.log(`No new trades to add for ${tickerKey} (all duplicates)`);
            return;
        }
        
        console.log(`➕ Adding ${newTrades.length} new trades to fill gap for ${tickerKey}`);
        
        // Merge new trades with existing trades
        const allTradesCombined = [...footprintState.allTrades, ...newTrades];
        
        // Sort by timestamp to ensure correct order
        allTradesCombined.sort((a, b) => a.timestamp - b.timestamp);
        
        // Remove duplicates (same timestamp) after sorting
        footprintState.allTrades = allTradesCombined.filter((trade, index, arr) => {
            if (index === 0) return true;
            return trade.timestamp !== arr[index - 1].timestamp;
        });
        
        console.log(`📊 After merge: ${footprintState.allTrades.length} total trades`);
        
        // Trim if too many trades (keep most recent for 7-day retention)
        if (footprintState.allTrades.length > 100000) {
            footprintState.allTrades = footprintState.allTrades.slice(-80000);
        }
        
        // Re-aggregate all trades to rebuild bars with the new data
        reAggregateTradesForBackup(footprintState);
        
        // Save the updated data
        saveFootprintData(tickerKey, footprintState);
        
        console.log(`✅ Backup data merged for ${tickerKey}: ${footprintState.bars.length} bars, ${footprintState.allTrades.length} trades`);
        
    } catch (e) {
        console.error(`Failed to fetch backup trades for ${tickerKey}:`, e);
    }
}

// Re-aggregate trades after backup data is added
function reAggregateTradesForBackup(footprintState) {
    if (footprintState.allTrades.length === 0) return;
    
    // Clear existing bars and rebuild
    footprintState.bars = [];
    footprintState.currentBar = null;
    footprintState.tickBuffer = [];
    
    const tickCount = footprintState.tickCount;
    const tickSize = footprintState.tickSize;
    let buffer = [];
    
    footprintState.allTrades.forEach((trade) => {
        buffer.push(trade);
        
        if (buffer.length >= tickCount) {
            const bar = createFootprintBarFromTrades(buffer, tickSize);
            if (bar) footprintState.bars.push(bar);
            buffer = [];
        }
    });
    
    // Keep remaining trades in buffer for next bar
    footprintState.tickBuffer = buffer;
    
    // Limit bars
    if (footprintState.bars.length > 200) {
        footprintState.bars = footprintState.bars.slice(-150);
    }
    
    // Render if we have a render function
    if (footprintState.renderFootprint) {
        footprintState.renderFootprint();
    }
}

// Create footprint bar from trades array (standalone version for backup)
function createFootprintBarFromTrades(ticks, tickSize) {
    if (ticks.length === 0) return null;
    
    const open = ticks[0].price;
    const close = ticks[ticks.length - 1].price;
    const high = Math.max(...ticks.map(t => t.price));
    const low = Math.min(...ticks.map(t => t.price));
    
    // Group trades by price level
    const priceLevels = {};
    const roundToTick = (p) => Math.round(p / tickSize) * tickSize;
    
    ticks.forEach(trade => {
        const level = roundToTick(trade.price);
        if (!priceLevels[level]) {
            priceLevels[level] = { bid: 0, ask: 0, delta: 0 };
        }
        if (trade.is_buyer_maker) {
            priceLevels[level].bid += trade.quantity || 1;
        } else {
            priceLevels[level].ask += trade.quantity || 1;
        }
        priceLevels[level].delta = priceLevels[level].ask - priceLevels[level].bid;
    });
    
    // Find POC
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
        time: ticks[ticks.length - 1].timestamp || Date.now(),
        open, high, low, close,
        priceLevels,
        pocPrice,
        isBullish: close >= open,
    };
}

// ============================================================================
// TAB NAVIGATION AND HEALTH DASHBOARD
// ============================================================================

let healthRefreshInterval = null;

// Switch between tabs
function switchTab(tabName) {
    // Update tab buttons
    document.querySelectorAll('.main-tab').forEach(tab => {
        tab.classList.toggle('active', tab.dataset.tab === tabName);
    });
    
    // Update tab content
    document.querySelectorAll('.tab-content').forEach(content => {
        content.classList.remove('active');
    });
    
    if (tabName === 'charts') {
        document.getElementById('chartsView').classList.add('active');
        stopHealthRefresh();
    } else if (tabName === 'health') {
        document.getElementById('healthView').classList.add('active');
        loadHealthData();
        startHealthRefresh();
    } else if (tabName === 'api') {
        document.getElementById('apiView').classList.add('active');
        stopHealthRefresh();
        renderApiDashboard();
    }
}

// Start health data auto-refresh
function startHealthRefresh() {
    if (healthRefreshInterval) return;
    healthRefreshInterval = setInterval(loadHealthData, 5000);
}

// Stop health data auto-refresh
function stopHealthRefresh() {
    if (healthRefreshInterval) {
        clearInterval(healthRefreshInterval);
        healthRefreshInterval = null;
    }
}

// Load health data from server
async function loadHealthData() {
    try {
        const response = await fetch(`${API_BASE}/api/v1/health/detailed`);
        if (!response.ok) throw new Error('Failed to fetch health data');
        
        const data = await response.json();
        renderHealthDashboard(data);
    } catch (error) {
        document.getElementById('healthContent').innerHTML = `
            <div class="health-error">
                <strong>Error:</strong> ${error.message}
            </div>
        `;
    }
}

// Format uptime for display
function formatUptime(seconds) {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;
    
    if (days > 0) return `${days}d ${hours}h ${minutes}m`;
    if (hours > 0) return `${hours}h ${minutes}m ${secs}s`;
    if (minutes > 0) return `${minutes}m ${secs}s`;
    return `${secs}s`;
}

// Format bytes for display
function formatBytes(bytes) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

// Render health dashboard
function renderHealthDashboard(data) {
    const isHealthy = data.status === 'ok';
    
    document.getElementById('healthContent').innerHTML = `
        <div class="health-status-banner ${isHealthy ? '' : 'error'}">
            <div class="health-status-icon">${isHealthy ? '✓' : '✗'}</div>
            <div class="health-status-info">
                <h2>Server ${isHealthy ? 'Healthy' : 'Unhealthy'}</h2>
                <p>Version ${data.version} • Uptime: ${formatUptime(data.uptime_seconds)}</p>
            </div>
            <div class="health-live-indicator">
                <span class="health-live-dot"></span>
                Live • Auto-refresh 5s
            </div>
        </div>
        
        <div class="health-stats-grid">
            <div class="health-stat-card">
                <h3>🔗 Connections</h3>
                <div class="health-stat-value green">${data.connections.active_websocket_subscribers}</div>
                <div class="health-stat-label">Active WebSocket Subscribers</div>
                <div class="health-stat-details">
                    <div class="health-stat-row">
                        <span class="label">Total Tickers</span>
                        <span class="value">${data.connections.total_tickers}</span>
                    </div>
                    <div class="health-stat-row">
                        <span class="label">Exchanges</span>
                        <span class="value">${data.connections.exchanges_connected.join(', ')}</span>
                    </div>
                </div>
            </div>
            
            <div class="health-stat-card">
                <h3>💾 Storage</h3>
                <div class="health-stat-value">${data.storage.footprint_files}</div>
                <div class="health-stat-label">Footprint Data Files</div>
                <div class="health-stat-details">
                    <div class="health-stat-row">
                        <span class="label">Total Size</span>
                        <span class="value">${formatBytes(data.storage.total_size_bytes)}</span>
                    </div>
                    <div class="health-stat-row">
                        <span class="label">Oldest Data</span>
                        <span class="value">${data.storage.oldest_file_age_hours.toFixed(1)}h ago</span>
                    </div>
                    <div class="health-stat-row">
                        <span class="label">Newest Data</span>
                        <span class="value">${data.storage.newest_file_age_hours.toFixed(1)}h ago</span>
                    </div>
                </div>
            </div>
            
            <div class="health-stat-card">
                <h3>🧠 Memory</h3>
                <div class="health-stat-value">${data.memory.trades_in_memory.toLocaleString()}</div>
                <div class="health-stat-label">Trades in Memory</div>
                <div class="health-stat-details">
                    <div class="health-stat-row">
                        <span class="label">Candles in Memory</span>
                        <span class="value">${data.memory.candles_in_memory.toLocaleString()}</span>
                    </div>
                    <div class="health-stat-row">
                        <span class="label">Symbols Tracked</span>
                        <span class="value">${data.memory.symbols_tracked}</span>
                    </div>
                </div>
            </div>
            
            <div class="health-stat-card">
                <h3>⏱️ Uptime</h3>
                <div class="health-stat-value green">${formatUptime(data.uptime_seconds)}</div>
                <div class="health-stat-label">Server Running</div>
                <div class="health-stat-details">
                    <div class="health-stat-row">
                        <span class="label">Server Time</span>
                        <span class="value">${new Date(data.server_time).toLocaleTimeString()}</span>
                    </div>
                    <div class="health-stat-row">
                        <span class="label">Status</span>
                        <span class="value" style="color: #26a69a;">● Online</span>
                    </div>
                </div>
            </div>
        </div>
        
        <div class="health-tickers-section">
            <div class="health-tickers-header">
                <h3>📈 Active Tickers (${data.active_tickers.length})</h3>
                <span class="health-refresh-info">Sorted by subscribers</span>
            </div>
            <table class="health-tickers-table">
                <thead>
                    <tr>
                        <th>Exchange</th>
                        <th>Symbol</th>
                        <th>Price</th>
                        <th>Trades</th>
                        <th>Subscribers</th>
                    </tr>
                </thead>
                <tbody>
                    ${data.active_tickers.map(ticker => `
                        <tr>
                            <td><span class="health-exchange-badge ${ticker.exchange}">${ticker.exchange}</span></td>
                            <td><strong>${ticker.symbol}</strong></td>
                            <td>$${formatPrice(ticker.price)}</td>
                            <td>${ticker.trades_count.toLocaleString()}</td>
                            <td>
                                <span class="health-subscriber-badge ${ticker.subscribers === 0 ? 'inactive' : ''}">
                                    ${ticker.subscribers > 0 ? '●' : '○'} ${ticker.subscribers}
                                </span>
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        </div>
    `;
}

// ============================================================================
// API CONFIGURATION DASHBOARD
// ============================================================================

// Render API dashboard with endpoints documentation
async function renderApiDashboard() {
    const serverUrl = window.location.origin;
    
    // Fetch API key from server
    let apiKey = 'Loading...';
    try {
        const response = await fetch(`${API_BASE}/api/v1/api-key`);
        const data = await response.json();
        apiKey = data.api_key;
    } catch (e) {
        apiKey = 'Error loading API key';
    }
    
    document.getElementById('apiContent').innerHTML = `
        <div class="api-header">
            <div class="api-header-icon">🔌</div>
            <div class="api-header-info">
                <h2>Sync API for Desktop Client</h2>
                <p>REST API endpoints for desktop app data synchronization</p>
            </div>
            <div class="api-server-url">${serverUrl}</div>
        </div>
        
        <div class="api-config-grid">
            <div class="api-config-card">
                <h3>📡 Server URL</h3>
                <div class="api-config-value">${serverUrl}</div>
            </div>
            <div class="api-config-card" style="grid-column: span 2;">
                <h3>🔑 API Key</h3>
                <div class="api-config-value" id="apiKeyDisplay" style="font-size: 14px; word-break: break-all;">${apiKey}</div>
                <div style="margin-top: 10px; display: flex; gap: 10px;">
                    <button class="api-try-btn" onclick="copyApiKey('${apiKey}')">📋 Copy</button>
                    <button class="api-try-btn" onclick="regenerateApiKey()" style="background: #ef5350;">� Regenerate Key</button>
                </div>
            </div>
            <div class="api-config-card">
                <h3>� Data Retention</h3>
                <div class="api-config-value">7 Days</div>
            </div>
        </div>
        
        <div class="api-section">
            <div class="api-section-header">
                <h3>�️ Desktop Client Configuration</h3>
            </div>
            <div class="api-endpoint">
                <div class="api-endpoint-desc">
                    <strong>Server URL:</strong> <code>${serverUrl}</code><br><br>
                    <strong>API Key:</strong> <code>${apiKey}</code><br><br>
                    <strong>Instructions:</strong><br>
                    1. Open your desktop charting app<br>
                    2. Go to Settings → Server Connection<br>
                    3. Enable "Enable history fetch"<br>
                    4. Enter Server URL: <code>${serverUrl}</code><br>
                    5. Enter API Key: <code>${apiKey}</code><br>
                    6. Click Save/Connect
                </div>
            </div>
        </div>
        
        <div class="api-section">
            <div class="api-section-header">
                <h3>📋 Sync Endpoints</h3>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/sync/tickers</span>
                <div class="api-endpoint-desc">Get list of all available tickers with sync status</div>
                <button class="api-try-btn" onclick="tryApiEndpoint('/api/v1/sync/tickers', 'sync-tickers-response')">Try It</button>
                <div class="api-response" id="sync-tickers-response" style="display:none;"></div>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/sync/{exchange}/{symbol}/latest</span>
                <div class="api-endpoint-desc">Get latest trade timestamp and counts for sync coordination</div>
                <div class="api-endpoint-params">
                    <strong>Response:</strong> <code>latest_timestamp</code>, <code>trades_count</code>, <code>bars_count</code>, <code>server_time</code>
                </div>
                <button class="api-try-btn" onclick="tryApiEndpoint('/api/v1/sync/binance/BTCUSDT/latest', 'sync-latest-response')">Try (BTCUSDT)</button>
                <div class="api-response" id="sync-latest-response" style="display:none;"></div>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/sync/{exchange}/{symbol}/trades</span>
                <div class="api-endpoint-desc">Get historical trades for desktop client sync</div>
                <div class="api-endpoint-params">
                    <strong>Query params:</strong> <code>since</code> (timestamp), <code>limit</code> (max 50000)
                </div>
                <button class="api-try-btn" onclick="tryApiEndpoint('/api/v1/sync/binance/BTCUSDT/trades?limit=100', 'sync-trades-response')">Try (100 trades)</button>
                <div class="api-response" id="sync-trades-response" style="display:none;"></div>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/sync/{exchange}/{symbol}/bars</span>
                <div class="api-endpoint-desc">Get pre-aggregated footprint bars for desktop client sync</div>
                <div class="api-endpoint-params">
                    <strong>Query params:</strong> <code>since</code> (timestamp), <code>limit</code> (max 500)<br>
                    <strong>Response includes:</strong> <code>tick_count</code>, <code>tick_size</code>, <code>bars[]</code>
                </div>
                <button class="api-try-btn" onclick="tryApiEndpoint('/api/v1/sync/binance/BTCUSDT/bars?limit=10', 'sync-bars-response')">Try (10 bars)</button>
                <div class="api-response" id="sync-bars-response" style="display:none;"></div>
            </div>
        </div>
        
        <div class="api-section">
            <div class="api-section-header">
                <h3>🔧 Other Endpoints</h3>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/health</span>
                <div class="api-endpoint-desc">Basic health check</div>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/health/detailed</span>
                <div class="api-endpoint-desc">Detailed server health with connections, storage, memory stats</div>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/tickers</span>
                <div class="api-endpoint-desc">Get all available tickers from all exchanges</div>
            </div>
            
            <div class="api-endpoint">
                <span class="api-endpoint-method get">GET</span>
                <span class="api-endpoint-path">/api/v1/backup-trades/{exchange}/{symbol}</span>
                <div class="api-endpoint-desc">Fetch recent trades from exchange REST API (for gap filling)</div>
                <div class="api-endpoint-params">
                    <strong>Query params:</strong> <code>limit</code> (max 1000)
                </div>
            </div>
        </div>
        
        <div class="api-section">
            <div class="api-section-header">
                <h3>📖 Desktop Client Integration</h3>
            </div>
            <div class="api-endpoint">
                <div class="api-endpoint-desc">
                    <strong>Sync Flow for Desktop Client:</strong><br><br>
                    1. Call <code>GET /api/v1/sync/{exchange}/{symbol}/latest</code> to get server's latest timestamp<br><br>
                    2. Start exchange WebSocket connection for live trades (from current time forward)<br><br>
                    3. Call <code>GET /api/v1/sync/{exchange}/{symbol}/trades?since=0</code> to get historical trades<br><br>
                    4. Merge historical trades with live trades, deduplicate by timestamp<br><br>
                    5. Build continuous footprint chart with no gaps
                </div>
            </div>
        </div>
    `;
}

// Copy API key to clipboard
function copyApiKey(apiKey) {
    navigator.clipboard.writeText(apiKey).then(() => {
        alert('API Key copied to clipboard!');
    }).catch(err => {
        console.error('Failed to copy:', err);
        prompt('Copy this API Key:', apiKey);
    });
}

// Regenerate API key (invalidates old key)
async function regenerateApiKey() {
    if (!confirm('⚠️ WARNING: This will invalidate the current API key!\n\nAll desktop clients using the old key will be disconnected.\n\nAre you sure you want to regenerate the API key?')) {
        return;
    }
    
    try {
        const response = await fetch(`${API_BASE}/api/v1/api-key/regenerate`, {
            method: 'POST'
        });
        const data = await response.json();
        
        if (data.success) {
            alert('✅ API Key regenerated successfully!\n\nNew key: ' + data.api_key + '\n\nPlease update your desktop client with the new key.');
            // Refresh the API dashboard to show new key
            renderApiDashboard();
        } else {
            alert('❌ Failed to regenerate API key: ' + (data.error || 'Unknown error'));
        }
    } catch (e) {
        alert('❌ Error regenerating API key: ' + e.message);
    }
}

// Try API endpoint and display response
async function tryApiEndpoint(endpoint, responseId) {
    const responseEl = document.getElementById(responseId);
    responseEl.style.display = 'block';
    responseEl.textContent = 'Loading...';
    
    try {
        const response = await fetch(endpoint);
        const data = await response.json();
        responseEl.textContent = JSON.stringify(data, null, 2);
    } catch (e) {
        responseEl.textContent = 'Error: ' + e.message;
        responseEl.style.color = '#ef5350';
    }
}
