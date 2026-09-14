
        new Vue({
            el: '#app',
            data: {
                searchQuery: '', prevCategory: null, loadingSearch: false,
                searchActive: false,
                scrolled: false,
                allResults: [],
                loading: false,
                selectedItem: null,
                showDetailsModal: false,
                loadingDetails: false,
                streamsError: null,
                loadingStreams: false,
                showQualityPicker: false,
                qualityPickerLoading: false,
                availableQualities: [],
                pendingAction: null,
                activeSeason: null,
                streams: [],
                streamQualities: [],
                selectedQuality: [],
                browserPlayUrl: null,
                hls: null,
                dash: null,
                player: null,
                isMobile: /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent),
                category: 'Trending',
                currentPage: 1,
                itemsPerPage: 24,
                showVlcModal: false,
                vlcSubtitles: [],
                selectedSubtitle: null,
                pendingVlcMirror: null,
                historyItems: [],
                favoriteItems: [],
                currentHeroIndex: 0,
                heroInterval: null,
                filterYear: '',
                filterType: '',
                filterGenre: '',
                playerProgressInterval: null,
                isFavorite: false,
                serverPage: 1,
                hasMoreServerPages: true,
                loadingMore: false,
                loadingLists: false,
                selectedProvider: 'moviebox',
                providers: [
                    { id: 'moviebox', name: 'MovieBox' },
                    { id: 'fourkhdhub', name: '4KHDHub' },
                    { id: 'bdix_circleftp', name: 'CircleFTP (BDIX)' },
                    { id: 'bdix_dhakaflix', name: 'DhakaFlix (BDIX)' }
                ],
                suggestResults: [],
                suggestStrings: [],
                showSuggest: false,
                suggestIndex: -1,
                suggestTimer: null,
                suggestReqId: 0,
                recommendedItems: [],
                loadingRecs: false,
                downloadingIdx: null,
                toastMsg: null,
                toastIsError: false,
                user: null,
                authLoading: true,
                loginRequired: false,
                dlJobs: [],
                showDownloads: false,
                dlPollTimer: null,
                dlLoading: false,
                showLoginModal: false,
                showUserMenu: false,
                detailsError: null,
                streamsError: null,
                gridError: null,
                playerError: null,
                subtitleCount: 0,
                selectedMirror: {},
                lastStreamsKey: null,
                lastPlayback: null,
                pendingDetailsItem: null,
                visibleCount: 30,
                infiniteObs: null
            },
            computed: {
                historyItemsMapped() {
                    return this.historyItems.map(h => ({
                        id: { value: h.subject_id, provider: h.provider },
                        title: h.title,
                        poster_url: h.cover_url,
                        media_type: h.stype === 1 ? 'movie' : 'series',
                        year: h.release_year,
                        season: h.season,
                        episode: h.episode
                    }));
                },
                favoriteItemsMapped() {
                    return this.favoriteItems.map(f => ({
                        id: { value: f.subject_id, provider: f.provider },
                        title: f.title,
                        poster_url: f.cover_url,
                        media_type: f.stype === 1 ? 'movie' : 'series',
                        year: f.release_year
                    }));
                },
                filteredResults() {
                    let res = this.allResults;
                    if (this.category === 'Movies' || this.filterType === 'movie') {
                        res = res.filter(item => item.media_type === 'movie');
                    } else if (this.category === 'Series' || this.filterType === 'series') {
                        res = res.filter(item => item.media_type === 'series');
                    }
                    if (this.category === 'History') {
                        res = this.historyItemsMapped;
                    }
                    if (this.category === 'My List') {
                        res = this.favoriteItemsMapped;
                    }
                    if (this.filterYear) {
                        res = res.filter(item => item.year === this.filterYear);
                    }
                    if (this.filterGenre) {
                        res = res.filter(item => item.genre && item.genre.toLowerCase().includes(this.filterGenre.toLowerCase()));
                    }
                    return res;
                },
                visibleResults() {
                    return this.filteredResults.slice(0, this.visibleCount);
                },
                dlActiveCount() {
                    return this.dlJobs.filter(j => ['queued', 'downloading', 'merging'].includes(j.status)).length;
                },
                hasLibraryAccess() {
                    // Dev mode (or logged-in prod): library is open. Only a
                    // logged-out production user is gated.
                    return !this.loginRequired || !!this.user;
                },
                isPersonalCategory() {
                    return this.category === 'My List' || this.category === 'History';
                },
                categoryHasMore() {
                    // Personal lists are finite local arrays: more means
                    // unrevealed client-side items. Server feeds page remotely.
                    if (this.isPersonalCategory) return this.visibleCount < this.filteredResults.length;
                    return this.visibleCount < this.filteredResults.length || this.hasMoreServerPages;
                },
                emptyStateTitle() {
                    if (this.category === 'My List') return this.hasLibraryAccess ? 'Your list is empty' : 'Sign in to see your list';
                    if (this.category === 'History') return this.hasLibraryAccess ? 'No watch history yet' : 'Sign in to see history';
                    return 'No results found';
                },
                emptyStateHint() {
                    if ((this.category === 'My List' || this.category === 'History') && !this.hasLibraryAccess) return 'Sign in with Google to sync across devices.';
                    if (this.category === 'My List') return 'Tap + My List on any title to save it here.';
                    if (this.category === 'History') return 'Titles you play will appear here to resume.';
                    return 'Try another provider or clear filters.';
                },
                heroItems() {
                    return this.allResults.slice(0, 5);
                },
                heroItem() {
                    return this.heroItems.length > 0 ? this.heroItems[this.currentHeroIndex % this.heroItems.length] : null;
                },
                availableYears() {
                    const years = new Set();
                    this.allResults.forEach(item => {
                        if (item.year) years.add(item.year);
                    });
                    return Array.from(years).sort().reverse();
                },
                availableGenres() {
                    const genres = new Set();
                    this.allResults.forEach(item => {
                        if (item.genre) {
                            item.genre.split(',').forEach(g => {
                                const clean = g.trim();
                                if (clean) genres.add(clean);
                            });
                        }
                    });
                    return Array.from(genres).sort();
                }
            },
            mounted() {
                this.fetchHomepage();
                this.fetchRecommendations();
                this.fetchDlJobs(true).then(() => { if (this.dlActiveCount > 0) this.ensureDlPolling(); });
                this.fetchMe().then(() => {
                    if (!this.user) return;
                    this.fetchHistory();
                    this.fetchFavorites();
                    this.fetchRecommendations();
                });
                window.addEventListener('scroll', this.handleScroll, { passive: true });
                window.addEventListener('keydown', (e) => { if (e.key === 'Escape') { this.showUserMenu = false; } });
                this.startHeroInterval();
                this.setupInfinite();
            },
            beforeDestroy() {
                window.removeEventListener('scroll', this.handleScroll);
                this.stopHeroInterval();
                this.stopDlPolling();
                if (this.infiniteObs) this.infiniteObs.disconnect();
            },
            watch: {
                category() {
                    this.visibleCount = 30;
                    this.filterYear = '';
                    this.filterType = '';
                    this.filterGenre = '';
                },
                searchQuery() {
                    this.currentPage = 1;
                },
                selectedProvider() {
                    this.currentPage = 1;
                    this.fetchHomepage(false);
                    if ((this.searchQuery || '').trim().length >= 2) this.onSuggestInput();
                }
            },
            methods: {
                requireLogin() {
                    if (!this.loginRequired) return true;
                    if (this.user) return true;
                    this.showLoginModal = true;
                    return false;
                },
                authFailed(detail) {
                    // Production: session expired/invalid -> force Google login.
                    // Development: a 401 means the backend is stale (old binary
                    // still enforcing auth) -> NEVER show Google login; tell the
                    // user to restart the server instead.
                    if (this.loginRequired) {
                        this.user = null;
                        this.showLoginModal = true;
                    } else {
                        this.showToast(detail || 'Server blocked the request — restart it with: cargo run --bin server', true);
                    }
                },
                apiErrorMessage(res, fallback) {
                    return res.text().then(t => {
                        try { const j = JSON.parse(t); return j.hint || j.error || fallback; }
                        catch (e) { return t || fallback; }
                    }).catch(() => fallback);
                },
                matchPercent(rating) {
                    const r = parseFloat(rating);
                    if (isNaN(r)) return null;
                    return Math.round((r / 10) * 100);
                },
                formatBytes(n) {
                    if (!n || n <= 0) return '';
                    const u = ['B', 'KB', 'MB', 'GB'];
                    let i = 0; let v = n;
                    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
                    return (v >= 100 ? Math.round(v) : v.toFixed(1)) + u[i];
                },
                activeMirror(stream, idx) {
                    if (!stream || !stream.mirrors) return null;
                    return stream.mirrors[this.selectedMirror[idx] || 0] || stream.mirrors[0];
                },
                isActiveDub(d) {
                    return this.selectedItem && this.selectedItem._activeDubId && d.subject_id && this.selectedItem._activeDubId === d.subject_id;
                },
                async switchDub(d) {
                    if (!d || !d.subject_id || !this.selectedItem) return;
                    const provider = this.selectedItem.id.provider;
                    this.selectedItem._activeDubId = d.subject_id;
                    await this.fetchDetails({ id: { value: d.subject_id, provider }, title: d.label || d.language || this.selectedItem.title });
                },
                episodeProgress(season, episode) {
                    if (!this.selectedItem) return 0;
                    const h = this.historyItems.find(x => x.subject_id === this.selectedItem.id.value && x.provider === this.selectedItem.id.provider && x.season === season && x.episode === episode);
                    if (!h || !h.duration_seconds) return 0;
                    return Math.min(100, Math.round((h.progress_seconds / h.duration_seconds) * 100));
                },
                setupInfinite() {
                    this.$nextTick(() => {
                        const el = this.$refs.infiniteSentinel;
                        if (!el || !('IntersectionObserver' in window)) return;
                        if (this.infiniteObs) this.infiniteObs.disconnect();
                        this.infiniteObs = new IntersectionObserver((entries) => {
                            if (entries.some(e => e.isIntersecting)) this.loadMoreInfinite();
                        }, { rootMargin: '600px' });
                        this.infiniteObs.observe(el);
                    });
                },
                loadMoreInfinite() {
                    if (this.loading || this.loadingMore) return;
                    // Personal lists are finite: reveal client-side items only,
                    // never hit the homepage feed from My List / History.
                    if (this.visibleCount < this.filteredResults.length) {
                        this.visibleCount += 30;
                        return;
                    }
                    if (this.isPersonalCategory || !this.hasMoreServerPages) return;
                    this.fetchHomepage(true);
                },
                retryGrid() { this.gridError = null; this.fetchHomepage(false); },
                retryDetails() { if (this.pendingDetailsItem) this.fetchDetails(this.pendingDetailsItem); },
                retryStreams() {
                    this.streamsError = null;
                    if (this.lastStreamsKey) {
                        const k = this.lastStreamsKey;
                        this.fetchStreams(k.id, k.provider, k.season, k.episode);
                    }
                },
                retryPlayback() {
                    this.playerError = null;
                    if (this.lastPlayback) {
                        const p = this.lastPlayback;
                        this.playInBrowser(p.stream, p.mirror);
                    }
                },
                startHeroInterval() {
                    this.heroInterval = setInterval(() => {
                        this.currentHeroIndex = (this.currentHeroIndex + 1) % 5;
                    }, 8000);
                },
                stopHeroInterval() {
                    if (this.heroInterval) clearInterval(this.heroInterval);
                },
                setHeroIndex(idx) {
                    this.currentHeroIndex = idx;
                    this.stopHeroInterval();
                    this.startHeroInterval();
                },
                async fetchMe() {
                    this.authLoading = true;
                    try {
                        const c = await fetch('/api/config');
                        if (c.ok) {
                            const cfg = await c.json();
                            this.loginRequired = !!cfg.login_required;
                        }
                    } catch (e) {}
                    try { const r = await fetch('/api/auth/me'); this.user = r.ok ? await r.json() : null; }
                    catch (e) { this.user = null; }
                    finally { this.authLoading = false; }
                },
                async logout() {
                    try { await fetch('/api/auth/logout', { method: 'POST' }); } catch (e) {}
                    this.user = null; this.historyItems = []; this.favoriteItems = []; this.recommendedItems = [];
                    this.dlJobs = []; this.showDownloads = false; this.stopDlPolling();
                    this.allResults = []; this.gridError = null; this.suggestResults = []; this.suggestStrings = [];
                    this.showUserMenu = false; this.showLoginModal = false; this.category = 'Trending';
                    this.closeDetails();
                    window.scrollTo({ top: 0 });
                },
                async fetchHistory() {
                    if (this.loginRequired && !this.user) { this.historyItems = []; return; }
                    this.loadingLists = true;
                    try {
                        const res = await fetch('/api/history');
                        if (res.status === 401) { this.historyItems = []; this.authFailed(); return; }
                        if (res.ok) { this.historyItems = await res.json(); this.fetchRecommendations(); }
                    } catch (e) {}
                    finally { this.loadingLists = false; }
                },
                async fetchFavorites() {
                    if (this.loginRequired && !this.user) { this.favoriteItems = []; return; }
                    this.loadingLists = true;
                    try {
                        const res = await fetch('/api/favorites');
                        if (res.status === 401) { this.favoriteItems = []; this.authFailed(); return; }
                        if (res.ok) { this.favoriteItems = await res.json(); this.fetchRecommendations(); }
                    } catch (e) {}
                    finally { this.loadingLists = false; }
                },
                async fetchRecommendations() {
                    this.loadingRecs = true;
                    try {
                        const res = await fetch('/api/recommendations?limit=24');
                        if (res.status === 401) { this.recommendedItems = []; this.authFailed(); return; }
                        if (res.ok) this.recommendedItems = await res.json();
                    } catch (e) {}
                    this.loadingRecs = false;
                },
                onSuggestInput() {
                    clearTimeout(this.suggestTimer);
                    const q = (this.searchQuery || '').trim();
                    if (q.length === 0) { this.clearSearch(); return; }
                    if (this.category !== 'Search') this.prevCategory = this.category;
                    this.category = 'Search';
                    this.allResults = [];
                    this.loadingSearch = true;
                    this.suggestTimer = setTimeout(() => this.executeSearch(q), 500);
                },
                onSuggestFocus() {
                    if (this.suggestResults.length || this.suggestStrings.length) this.showSuggest = true;
                    else if ((this.searchQuery || '').trim().length >= 2) this.onSuggestInput();
                },
                hideSuggestDelayed() { setTimeout(() => { this.showSuggest = false; }, 200); },
                async executeSearch(q) {
                    try {
                        const res = await fetch(`/api/search?q=${encodeURIComponent(q)}`);
                        const data = await res.json();
                        this.allResults = data.results || [];
                    } catch (e) { this.gridError = e.message; }
                    finally { this.loadingSearch = false; }
                },
                clearSearch() {
                    this.searchQuery = "";
                    this.searchActive = false;
                    if (this.category === 'Search') this.category = this.prevCategory || 'Trending';
                },
                async fetchSuggest(q) {
                    const myId = ++this.suggestReqId;
                    try {
                        const res = await fetch(`/api/suggest?q=${encodeURIComponent(q)}`);
                        if (res.status === 401) { this.authFailed(); return; }
                        if (!res.ok) return;
                        const data = await res.json();
                        if (myId !== this.suggestReqId) return;
                        this.suggestResults = data.results || [];
                        this.suggestStrings = data.suggestions || [];
                        this.suggestIndex = -1;
                        this.showSuggest = true;
                    } catch (e) {}
                },
                moveSuggest(dir) {
                    const n = this.suggestResults.length || this.suggestStrings.length;
                    if (!n) return;
                    this.suggestIndex = (this.suggestIndex + dir + n) % n;
                    this.showSuggest = true;
                },
                onSuggestEnter() {
                    if (this.showSuggest && this.suggestIndex >= 0 && this.suggestResults[this.suggestIndex]) {
                        this.pickSuggest(this.suggestResults[this.suggestIndex]);
                    } else if (this.suggestResults.length > 0) {
                        this.pickSuggest(this.suggestResults[0]);
                    } else {
                        this.showToast('No matches yet — keep typing.', true);
                    }
                },
                pickSuggest(item) {
                    if (!item) return;
                    this.showSuggest = false;
                    this.searchActive = false;
                    this.fetchDetails(item);
                },
                async runStringSuggest(s) {
                    this.searchQuery = s;
                    this.showSuggest = false;
                    await this.fetchSuggest(s);
                    if (this.suggestResults.length > 0) this.pickSuggest(this.suggestResults[0]);
                },
                getDownloadUrl(mirror, stream) {
                    const headersStr = JSON.stringify(mirror.headers || []);
                    const base = ((this.selectedItem && this.selectedItem.title) ? this.selectedItem.title : (stream && stream.filename ? stream.filename : 'video')).replace(/\.[a-z0-9]+$/i, '');
                    return `/api/download?url=${encodeURIComponent(mirror.resolver_url)}&headers=${encodeURIComponent(headersStr)}&filename=${encodeURIComponent(base)}`;
                },
                showToast(msg, isError = false) {
                    this.toastMsg = msg; this.toastIsError = isError;
                    clearTimeout(this._toastTimer);
                    this._toastTimer = setTimeout(() => { this.toastMsg = null; }, 4500);
                },
                toggleDownloads() {
                    this.showDownloads = !this.showDownloads;
                    if (this.showDownloads) {
                        this.fetchDlJobs();
                        this.ensureDlPolling();
                    } else if (this.dlActiveCount === 0) {
                        this.stopDlPolling();
                    }
                },
                ensureDlPolling() {
                    if (this.dlPollTimer) return;
                    this.dlPollTimer = setInterval(async () => {
                        await this.fetchDlJobs(true);
                        if (!this.showDownloads && this.dlActiveCount === 0) this.stopDlPolling();
                    }, 1000);
                },
                stopDlPolling() {
                    if (this.dlPollTimer) { clearInterval(this.dlPollTimer); this.dlPollTimer = null; }
                },
                async fetchDlJobs(quiet = false) {
                    if (!quiet) this.dlLoading = true;
                    try {
                        const res = await fetch('/api/downloads');
                        if (res.status === 401) {
                            if (!quiet) this.authFailed();
                            return;
                        }
                        if (res.ok) this.dlJobs = await res.json();
                    } catch (e) {
                        if (!quiet) this.showToast('Could not load downloads.', true);
                    }
                    this.dlLoading = false;
                },
                isDlActive(job) { return ['queued', 'downloading', 'merging'].includes(job.status); },
                isDlDone(job) { return ['completed', 'failed', 'cancelled'].includes(job.status); },
                dlStatusLabel(job) {
                    const m = { queued: 'Queued', downloading: 'Downloading', merging: 'Merging', completed: 'Done', failed: 'Failed', cancelled: 'Cancelled' };
                    return m[job.status] || job.status;
                },
                dlStatusText(job) {
                    if (job.status === 'failed') return job.error || 'Download failed.';
                    if (job.status === 'cancelled') return 'Cancelled.';
                    if (job.status === 'completed') {
                        const size = job.total || job.downloaded;
                        const base = size ? this.formatBytes(size) + ' · saved on server — hit Save' : 'Done — hit Save';
                        return job.error ? base + ' (' + job.error + ')' : base;
                    }
                    // Active: same shape as the TUI status line (size · speed · ETA).
                    const bits = [];
                    if (job.total) bits.push(this.formatBytes(job.downloaded) + ' of ' + this.formatBytes(job.total));
                    else if (job.downloaded) bits.push(this.formatBytes(job.downloaded) + ' downloaded');
                    else bits.push((job.progress || 0).toFixed(1) + '%');
                    if (job.status === 'merging') bits.push('Merging audio & video…');
                    else {
                        if (job.speed_bps > 0) bits.push(this.formatSpeed(job.speed_bps));
                        if (job.eta_secs != null && job.eta_secs > 0) bits.push('ETA ' + this.formatEta(job.eta_secs));
                        else if (job.total) bits.push((job.progress || 0).toFixed(1) + '%');
                    }
                    return bits.join(' · ');
                },
                formatSpeed(bps) {
                    if (!bps || bps <= 0) return '';
                    return this.formatBytes(bps) + '/s';
                },
                formatEta(s) {
                    s = Math.max(0, Math.floor(s || 0));
                    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
                    const p = (n) => String(n).padStart(2, '0');
                    return h > 0 ? h + ':' + p(m) + ':' + p(sec) : p(m) + ':' + p(sec);
                },
                async downloadStream(stream, mirror, idx, selectedQuality) {
                    if (!this.requireLogin()) return;
                    if (!mirror) return;
                    if (this.downloadingIdx !== null) return;
                    this.downloadingIdx = idx;
                    try {
                        const headersArr = mirror.headers || [];
                        const base = ((this.selectedItem && this.selectedItem.title) ? this.selectedItem.title : (stream && stream.filename ? stream.filename : 'video')).replace(/\.[a-z0-9]+$/i, '');
                        const res = await fetch('/api/downloads', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ url: mirror.resolver_url, headers: headersArr, filename: base, quality: selectedQuality })
                        });
                        if (res.status === 401) { this.authFailed(); return; }
                        if (!res.ok) { this.showToast('Download failed: ' + await this.apiErrorMessage(res, 'Could not start download.'), true); return; }
                        const job = await res.json();
                        this.showToast('Download started: ' + job.filename, false);
                        this.showDownloads = true;
                        this.ensureDlPolling();
                        await this.fetchDlJobs(true);
                    } catch (e) { this.showToast('Network error. Could not start download.', true); }
                    finally { this.downloadingIdx = null; }
                },
                async cancelDownload(job) {
                    try {
                        const res = await fetch('/api/downloads/' + encodeURIComponent(job.id), { method: 'DELETE' });
                        if (res.status === 401) { this.authFailed(); return; }
                    } catch (e) {}
                    await this.fetchDlJobs(true);
                },
                async dismissDownload(job) { await this.cancelDownload(job); },
                async retryDownload(job) {
                    try {
                        const res = await fetch('/api/downloads/' + encodeURIComponent(job.id) + '/retry', { method: 'POST' });
                        if (res.status === 401) { this.authFailed(); return; }
                        if (!res.ok) { this.showToast('Retry failed: ' + await this.apiErrorMessage(res, 'Could not retry.'), true); return; }
                        this.ensureDlPolling();
                    } catch (e) { this.showToast('Network error.', true); }
                    await this.fetchDlJobs(true);
                },
                getHistoryProgress(item) {
                    const hItem = this.historyItems.find(h => h.subject_id === item.id.value && h.provider === item.id.provider);
                    if (!hItem || !hItem.duration_seconds) return 0;
                    return Math.min(100, Math.round((hItem.progress_seconds / hItem.duration_seconds) * 100));
                },
                checkFavoriteStatus() {
                    if (!this.selectedItem) return;
                    const id = this.selectedItem.id.value;
                    const provider = this.selectedItem.id.provider;
                    this.isFavorite = this.favoriteItems.some(f => f.subject_id === id && f.provider === provider);
                },
                async toggleFavorite() {
                    if (!this.selectedItem) return;
                    if (!this.requireLogin()) return;
                    
                    const title = this.selectedItem.title;
                    const stype = this.selectedItem.media_type === 'series' ? 2 : 1;
                    const release_year = this.selectedItem.year || '';
                    const cover_url = this.selectedItem.poster_url;
                    
                    const req = {
                        item: {
                            provider: this.selectedItem.id.provider,
                            subject_id: this.selectedItem.id.value,
                            title, cover_url, stype, release_year, added_at: 0
                        }
                    };
                    
                    try {
                        const res = await fetch('/api/favorites', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(req)
                        });
                        if (res.status === 401) { this.authFailed(); return; }
                        if (res.ok) {
                            const data = await res.json();
                            this.isFavorite = data.is_favorite;
                            await this.fetchFavorites();
                        } else {
                            this.showToast(await this.apiErrorMessage(res, 'Could not update My List.'), true);
                        }
                    } catch(e) { this.showToast('Network error. Check your connection.', true); }
                },
                handleScroll() {
                    this.scrolled = window.scrollY > 20;
                },
                async changePage(page) {
                    this.visibleCount = Math.max(30, page * 30);
                    window.scrollTo({ top: 0, behavior: 'smooth' });
                },
                toggleSearch() {
                    this.searchActive = !this.searchActive; if (this.searchActive) { this.$nextTick(() => this.$refs.searchInput.focus()); } else { this.clearSearch(); }
                    if (this.searchActive) {
                        this.$nextTick(() => {
                            const input = document.querySelector('input[type="text"]');
                            if (input) input.focus();
                        });
                    } else {
                        if (this.searchQuery) {
                            this.searchQuery = '';
                            this.category = 'Trending';
                            this.fetchHomepage();
                        }
                    }
                },
                goHome() {
                    this.searchQuery = '';
                    this.category = 'Trending';
                    this.searchActive = false;
                    this.fetchHomepage();
                    window.scrollTo({ top: 0, behavior: 'smooth' });
                },
                setCategory(cat) {
                    if (this.loginRequired && (cat === 'My List' || cat === 'History') && !this.user) {
                        this.showLoginModal = true;
                        return;
                    }
                    this.category = cat;
                    this.searchQuery = '';
                    this.searchActive = false;
                    this.visibleCount = 30;
                    if (this.allResults.length === 0 || cat === 'Trending') {
                        this.fetchHomepage();
                    }
                    // Re-observe: the sentinel remounts when the grid switches.
                    this.$nextTick(() => this.setupInfinite());
                },
                closeDetails() {
                    this.selectedItem = null;
                    this.showDetailsModal = false;
                    this.loadingDetails = false;
                    this.detailsError = null;
                    this.streamsError = null;
                    this.playerError = null;
                    this.subtitleCount = 0;
                    this.selectedMirror = {};
                    this.lastStreamsKey = null;
                    this.lastPlayback = null;
                    this.pendingDetailsItem = null;
                    this.activeSeason = null;
                    this.streams = [];
                    this.closePlayer();
                },
                closePlayer() {
                    this.browserPlayUrl = null;
                    if (this.playerProgressInterval) {
                        clearInterval(this.playerProgressInterval);
                        this.playerProgressInterval = null;
                    }
                    if (this.player) {
                        this.player.destroy();
                        this.player = null;
                    }
                    if (this.hls) {
                        this.hls.destroy();
                        this.hls = null;
                    }
                    if (this.dash) {
                        this.dash.reset();
                        this.dash = null;
                    }
                },
                async fetchHomepage(append = false) {
                    if (!append) {
                        this.loading = true;
                        this.gridError = null;
                        this.allResults = [];
                        this.serverPage = 1;
                        this.visibleCount = 30;
                        this.hasMoreServerPages = true;
                    } else {
                        if (this.loadingMore || !this.hasMoreServerPages) return;
                        this.serverPage++;
                        this.loadingMore = true;
                    }
                    try {
                        const res = await fetch(`/api/homepage?page=${this.serverPage}&provider=${encodeURIComponent(this.selectedProvider)}`);
                        if (res.status === 401) { this.loading = false; this.loadingMore = false; if (append && this.serverPage > 1) this.serverPage--; this.authFailed(); return; }
                        if (res.ok) {
                            const data = await res.json();
                            if (!data || data.length === 0) {
                                this.hasMoreServerPages = false;
                            }
                            if (append) {
                                const seen = new Set(this.allResults.map(x => x.id.provider + '::' + x.id.value));
                                for (const it of data) {
                                    const k = it.id.provider + '::' + it.id.value;
                                    if (!seen.has(k)) { seen.add(k); this.allResults.push(it); }
                                }
                            } else {
                                this.allResults = data;
                            }
                            this.$nextTick(() => this.setupInfinite());
                        } else {
                            if (!append) this.gridError = await this.apiErrorMessage(res, 'Could not load titles.');
                            else if (this.serverPage > 1) this.serverPage--;
                        }
                    } catch (e) {
                        if (!append) this.gridError = 'Network error. Check your connection and retry.';
                        else if (this.serverPage > 1) this.serverPage--;
                    }
                    this.loading = false;
                    this.loadingMore = false;
                },
                async search() {
                    // Dropdown-only search: full-page grid results were noisy/wrong,
                    // so Enter now just opens the best dropdown match.
                    const q = (this.searchQuery || '').trim();
                    if (!q) return;
                    if (this.suggestResults.length > 0) {
                        const idx = this.suggestIndex >= 0 ? this.suggestIndex : 0;
                        this.pickSuggest(this.suggestResults[idx]);
                        return;
                    }
                    await this.fetchSuggest(q);
                    if (this.suggestResults.length > 0) {
                        this.pickSuggest(this.suggestResults[0]);
                    } else {
                        this.showToast('No matches yet — keep typing.', true);
                    }
                },
                async fetchDetails(item) {
                    if (!item || !item.id) { this.showToast('Invalid title. Try another.', true); return; }
                    this.pendingDetailsItem = item;
                    this.closeDetails();
                    this.showDetailsModal = true;
                    this.loadingDetails = true;
                    this.detailsError = null;
                    try {
                        const res = await fetch(`/api/details?id=${encodeURIComponent(item.id.value)}&provider=${encodeURIComponent(item.id.provider)}`);
                        if (res.status === 401) { this.loadingDetails = false; this.showDetailsModal = false; this.authFailed('Please sign in to view this title.'); return; }
                        if (res.ok) {
                            this.selectedItem = await res.json();

                            if (!this.selectedItem.title) this.selectedItem.title = item.title;
                            if (!this.selectedItem.media_type) this.selectedItem.media_type = item.media_type;
                            if (!this.selectedItem.poster_url) this.selectedItem.poster_url = item.poster_url;

                            if (this.selectedItem.seasons && this.selectedItem.seasons.length > 0) {
                                this.activeSeason = this.selectedItem.seasons[0];
                            }

                            this.checkFavoriteStatus();
                            this.subtitleCount = 0;
                            this.selectedMirror = {};
                        } else {
                            this.detailsError = await this.apiErrorMessage(res, 'Failed to fetch details.');
                        }
                    } catch (e) {
                        this.detailsError = 'Network error. Check your connection and retry.';
                    }
                    this.loadingDetails = false;
                },
                async fetchStreams(id, provider, season, episode) {
                    if (!this.requireLogin()) return;
                    this.lastStreamsKey = { id, provider, season, episode };
                    this.streams = [];
                    this.streamsError = null;
                    this.closePlayer();
                    this.loadingStreams = true;
                    try {
                        const res = await fetch(`/api/streams?id=${encodeURIComponent(id)}&provider=${encodeURIComponent(provider)}&season=${season}&episode=${episode}`);
                        if (res.status === 401) { this.loadingStreams = false; this.authFailed('Please sign in to watch.'); return; }
                        if (res.ok) {
                            const data = await res.json();
                            this.streams = data || [];
                              this.streamQualities = new Array(this.streams.length).fill([]);
                              this.selectedQuality = new Array(this.streams.length).fill(null);
                              
                              // Background fetch qualities
                              this.streams.forEach((st, idx) => {
                                  const mirror = st.mirrors && st.mirrors.length > 0 ? st.mirrors[0] : st;
                                  if (!mirror) return;
                                  const proxyUrl = this.getProxyUrl(mirror);
                                  fetch(proxyUrl).then(r => r.text()).then(text => {
                                      let qs = [];
                                      if (proxyUrl.includes('.mpd')) qs = [...text.matchAll(/height="(\d+)"/g)].map(m => parseInt(m[1], 10));
                                      else if (proxyUrl.includes('.m3u8')) qs = [...text.matchAll(/RESOLUTION=\d+x(\d+)/g)].map(m => parseInt(m[1], 10));
                                      qs = [...new Set(qs.filter(h => !isNaN(h)))].sort((a,b) => b - a);
                                      if (qs.length > 0) {
                                          this.$set(this.streamQualities, idx, [...qs, 'Auto']);
                                          this.$set(this.selectedQuality, idx, qs[0]);
                                      }
                                  }).catch(() => {});
                              });
                            this.selectedMirror = {};
                            if (this.streams.length === 0) {
                                this.streamsError = 'No playable sources for this episode yet. Try another episode or provider.';
                            }
                        } else {
                            this.streamsError = await this.apiErrorMessage(res, 'Failed to find streams.');
                        }
                    } catch (e) {
                        this.streamsError = 'Network error. Check your connection and retry.';
                    }
                    this.loadingStreams = false;
                },
                getProxyUrl(mirror) {
                    const headersStr = JSON.stringify(mirror.headers || []);
                    return `/api/proxy?url=${encodeURIComponent(mirror.resolver_url)}&headers=${encodeURIComponent(headersStr)}`;
                },
                async openVlcModal(stream, mirror, idx) {
                    if (!this.requireLogin()) return;
                    if(!mirror) return;
                    this.pendingVlcMirror = mirror;
                    this.pendingVlcIdx = idx;
                    this.executeVlcLaunch();
                    return;
                    this.vlcSubtitles = [];
                    this.selectedSubtitle = null;
                    this.showVlcModal = true;
                    
                    if (stream && stream.resource_id) {
                        try {
                            const subRes = await fetch(`/api/subtitles?id=${encodeURIComponent(this.selectedItem.id.value)}&resource_id=${encodeURIComponent(stream.resource_id)}&season=${stream.season || 0}&episode=${stream.episode || 0}`);
                            if (subRes.ok) {
                                this.vlcSubtitles = await subRes.json();
                            }
                        } catch(e) {
                            console.error("Failed to load subtitles for VLC modal", e);
                        }
                    }
                },
                async executeVlcLaunch() {
                    if (!this.requireLogin()) return;
                    if(!this.pendingVlcMirror) return;

                    const headersStr = JSON.stringify(this.pendingVlcMirror.headers || []);
                    let proxyUrl = window.location.origin + `/api/proxy?url=${encodeURIComponent(this.pendingVlcMirror.resolver_url)}&headers=${encodeURIComponent(headersStr)}`;

                    if (this.isMobile) {
                        try {
                            this.launchingVlc = true;
                            let sidecarFetchUrl = `/api/sidecar?url=${encodeURIComponent(this.pendingVlcMirror.resolver_url)}&headers=${encodeURIComponent(headersStr)}`;
                            const sq = this.selectedQuality[this.pendingVlcIdx];
                            if (sq && sq !== 'Auto') sidecarFetchUrl += `&quality=${sq}`;
                            if (this.selectedSubtitle) {
                                sidecarFetchUrl += `&sub=${encodeURIComponent(this.selectedSubtitle)}`;
                            }
                            
                            const sidecarRes = await fetch(sidecarFetchUrl);
                            if (!sidecarRes.ok) throw new Error("Failed to spawn proxy");
                            
                            let sidecarUrl = await sidecarRes.text();
                            
                            // Ensure the IP points to the actual server, not 0.0.0.0 or localhost on the phone
                            sidecarUrl = sidecarUrl.replace('://0.0.0.0:', '://' + window.location.hostname + ':')
                                                   .replace('://127.0.0.1:', '://' + window.location.hostname + ':');

                            if (/iPad|iPhone|iPod/.test(navigator.userAgent)) {
                                window.location.href = "vlc://" + sidecarUrl;
                            } else {
                                const urlWithoutScheme = sidecarUrl.replace(/^https?:\/\//, '');
                                const scheme = sidecarUrl.startsWith('https') ? 'https' : 'http';
                                window.location.href = `intent://${urlWithoutScheme}#Intent;action=android.intent.action.VIEW;scheme=${scheme};type=video/*;end`;
                            }
                        } catch (e) {
                            console.error(e);
                            alert("Error generating mobile link");
                        } finally {
                            this.launchingVlc = false;
                            this.showVlcModal = false;
                        }
                        return;
                    }

                    try {
                        let playUrl = `/api/play?url=${encodeURIComponent(this.pendingVlcMirror.resolver_url)}&headers=${encodeURIComponent(headersStr)}`;
                        if (this.selectedSubtitle) {
                            playUrl += `&sub=${encodeURIComponent(this.selectedSubtitle)}`;
                        }
                        
                        this.launchingVlc = true;
                        const res = await fetch(playUrl);
                        if (!res.ok) {
                            alert("Failed to launch VLC on the server. Ensure VLC is installed.");
                        }
                    } catch (e) {
                        console.error(e);
                        alert("Error launching VLC.");
                    } finally {
                        this.launchingVlc = false;
                        this.showVlcModal = false;
                    }
                },
                promptQuality(stream, mirror, idx, type) {
                    if (!this.requireLogin()) return;
                    if (!mirror) return;
                    
                    this.pendingAction = { stream, mirror, idx, type };
                    this.showQualityPicker = true;
                    this.qualityPickerLoading = true;
                    this.availableQualities = [];
                    
                    const proxyUrl = this.getProxyUrl(mirror);
                    
                    fetch(proxyUrl).then(res => res.text()).then(text => {
                        let qualities = [];
                        if (proxyUrl.includes('.mpd')) {
                            const matches = [...text.matchAll(/height="(\d+)"/g)];
                            qualities = matches.map(m => parseInt(m[1], 10)).filter(h => !isNaN(h));
                        } else if (proxyUrl.includes('.m3u8')) {
                            const matches = [...text.matchAll(/RESOLUTION=\d+x(\d+)/g)];
                            qualities = matches.map(m => parseInt(m[1], 10)).filter(h => !isNaN(h));
                        }
                        qualities = [...new Set(qualities)].sort((a,b) => b - a);
                        
                        if (qualities.length > 0) {
                            this.availableQualities = qualities.map(q => q + 'p');
                        } else {
                            this.availableQualities = ['Auto'];
                        }
                        this.qualityPickerLoading = false;
                    }).catch(() => {
                        this.availableQualities = ['Auto'];
                        this.qualityPickerLoading = false;
                    });
                },
                confirmQuality(q) {
                    this.showQualityPicker = false;
                    let parsedQuality = null;
                    if (q !== 'Auto') {
                        parsedQuality = parseInt(q.replace('p', ''), 10);
                    }
                    
                    if (this.pendingAction) {
                        const { stream, mirror, idx, type } = this.pendingAction;
                        if (type === 'play') {
                            this.playInBrowser(stream, mirror, parsedQuality);
                        } else if (type === 'download') {
                            this.downloadStream(stream, mirror, idx, parsedQuality);
                        }
                        this.pendingAction = null;
                    }
                },
                async playInBrowser(stream, mirror, selectedQuality) {
                    if (!this.requireLogin()) return;
                    if(!mirror) return;
                    this.lastPlayback = { stream, mirror };
                    this.playerError = null;
                    this.closePlayer();
                    this.browserPlayUrl = this.getProxyUrl(mirror);
                    
                    this.$nextTick(async () => {
                        const video = document.getElementById('web-video');
                        if (!video) return;
                        
                        video.onerror = () => {
                            this.playerError = 'The video could not be loaded. The source may be offline — try another quality or VLC.';
                        };
                        if (stream && stream.resource_id) {
                            try {
                                const subRes = await fetch(`/api/subtitles?id=${encodeURIComponent(this.selectedItem.id.value)}&resource_id=${encodeURIComponent(stream.resource_id)}&season=${stream.season || 0}&episode=${stream.episode || 0}`);
                                if (subRes.status === 401) { this.authFailed('Please sign in to load subtitles.'); return; }
                                if (subRes.ok) {
                                    const subs = await subRes.json();
                                    this.subtitleCount = (subs || []).length;
                                    subs.forEach((sub) => {
                                        const track = document.createElement('track');
                                        track.kind = 'captions';
                                        track.label = sub.name;
                                        track.srclang = sub.name.substring(0, 2).toLowerCase();
                                        track.src = sub.url;
                                        video.appendChild(track);
                                    });
                                }
                            } catch(e) {}
                        }

                        const defaultOptions = {
                            controls: ['play-large', 'play', 'progress', 'current-time', 'mute', 'volume', 'captions', 'settings', 'pip', 'airplay', 'fullscreen'],
                            settings: ['captions', 'quality', 'speed'],
                            autoplay: true
                        };
                        
                        const initPlayer = () => {
                            this.player = new Plyr(video, defaultOptions);
                            
                            // Check if resuming from history
                            const historyMatch = this.historyItems.find(h => 
                                h.subject_id === this.selectedItem.id.value && 
                                h.season === (stream.season || 0) && 
                                h.episode === (stream.episode || 0)
                            );
                            
                            if (historyMatch && historyMatch.progress_seconds > 0) {
                                video.currentTime = historyMatch.progress_seconds;
                            }
                            
                            this.playerProgressInterval = setInterval(() => {
                                if (video.paused || !video.duration) return;
                                const progress = Math.floor(video.currentTime);
                                const duration = Math.floor(video.duration);
                                const completed = progress >= duration * 0.9; // 90% is completed
                                
                                const req = {
                                    item: {
                                        provider: this.selectedItem.id.provider,
                                        subject_id: this.selectedItem.id.value,
                                        title: this.selectedItem.title,
                                        cover_url: this.selectedItem.poster_url,
                                        stype: this.selectedItem.media_type === 'series' ? 2 : 1,
                                        release_year: this.selectedItem.year || '',
                                        season: stream.season || 0,
                                        episode: stream.episode || 0,
                                        timestamp: 0,
                                        progress_seconds: 0,
                                        completed: false
                                    },
                                    progress,
                                    duration,
                                    completed
                                };
                                
                                fetch('/api/history', {
                                    method: 'POST',
                                    headers: { 'Content-Type': 'application/json' },
                                    body: JSON.stringify(req)
                                }).catch(()=>{});
                            }, 10000); // every 10s
                        };

                        if (this.browserPlayUrl.includes('.mpd')) {
                            const proxyUrlParams = new URLSearchParams(this.browserPlayUrl.split('?')[1]);
                            const originalManifestUrl = proxyUrlParams.get('url');
                            const headersStr = proxyUrlParams.get('headers') || '[]';
                            
                            let originalBaseUrl = '';
                            if (originalManifestUrl) {
                                const urlObj = new URL(originalManifestUrl);
                                const parts = urlObj.pathname.split('/');
                                parts.pop();
                                urlObj.pathname = parts.join('/') + '/';
                                urlObj.search = ''; 
                                originalBaseUrl = urlObj.href;
                            }

                            fetch(this.browserPlayUrl).then(res => res.text()).then(manifestText => {
                                let modifiedManifest = manifestText;
                                const mpdMatch = modifiedManifest.match(/<MPD[^>]*>/i);
                                if (mpdMatch && originalBaseUrl) {
                                    const insertionIndex = mpdMatch.index + mpdMatch[0].length;
                                    modifiedManifest = modifiedManifest.slice(0, insertionIndex) + 
                                        `\n  <BaseURL>${originalBaseUrl}</BaseURL>\n` + 
                                        modifiedManifest.slice(insertionIndex);
                                }
                                
                                const blob = new Blob([modifiedManifest], { type: 'application/dash+xml' });
                                const blobUrl = URL.createObjectURL(blob);

                                initPlayer(); // Initialize Plyr BEFORE dash.js to prevent DOM detachment issues

                                this.dash = dashjs.MediaPlayer().create();
                                this.dash.extend("RequestModifier", function () {
                                    return {
                                        modifyRequestURL: function (url) {
                                            if (url.startsWith("blob:") || url.includes("/api/proxy?url=")) {
                                                return url;
                                            }
                                            return `/api/proxy?url=${encodeURIComponent(url)}&headers=${encodeURIComponent(headersStr)}`;
                                        }
                                    };
                                }, true);
                                
                                this.dash.initialize(video, blobUrl, false);
                                
                                this.dash.on(dashjs.MediaPlayer.events.STREAM_INITIALIZED, () => {
                                    const bitrates = this.dash.getBitrateInfoListFor("video");
                                    
                                    if (bitrates.length === 0) {
                                        this.playerError = 'Unsupported video codec (likely HEVC/H.265). Your browser can only play the audio. Please use the "Play in VLC" button instead.';
                                        return;
                                    }

                                    const availableQualities = bitrates.map(b => b.height).filter((h, i, a) => a.indexOf(h) === i).sort((a, b) => b - a);
                                    
                                    if (availableQualities.length > 0 && this.player) {
                                        const defQual = selectedQuality && availableQualities.includes(selectedQuality) ? selectedQuality : availableQualities[0];
                                        
                                        this.player.options.quality = {
                                            default: defQual,
                                            options: availableQualities,
                                            forced: true,
                                            onChange: (newQuality) => {
                                                const cfg = { streaming: { abr: { autoSwitchBitrate: { video: false } } } };
                                                this.dash.updateSettings(cfg);
                                                const match = this.dash.getBitrateInfoListFor("video").findIndex(b => b.height === newQuality);
                                                if (match !== -1) {
                                                    this.dash.setQualityFor("video", match);
                                                }
                                            }
                                        };
                                        if (defQual) {
                                            this.player.options.quality.onChange(defQual);
                                        }
                                    }
                                    
                                    this.player.play();
                                });
                                
                                this.dash.on(dashjs.MediaPlayer.events.ERROR, (e) => {
                                    let errStr = 'fatal';
                                    if (e && e.error) {
                                        errStr = e.error.message || e.error.code || e.error;
                                        if (typeof errStr === 'object') errStr = e.error.code || 'manifest/download error';
                                    } else if (e && e.type) {
                                        errStr = e.type;
                                    }
                                    if (errStr !== 'fatal') {
                                        this.playerError = 'Stream error (' + errStr + '). Try another quality or VLC.';
                                    }
                                });

                                video.addEventListener('error', (e) => {
                                    if (video.error && video.error.code === 3) {
                                        this.playerError = 'Video decode error. This stream uses an unsupported codec (like HEVC) for your browser. Please use VLC.';
                                    }
                                });
                            }).catch(() => {
                                this.playerError = 'Stream error (manifest fetch failed). Try another quality or VLC.';
                            });
                        } else if (Hls.isSupported()) {
                            this.hls = new Hls();
                            this.hls.on(Hls.Events.ERROR, (event, data) => {
                                if (data && data.fatal) {
                                    this.playerError = 'Stream error (' + (data.details || data.type || 'fatal') + '). Try another quality or VLC.';
                                }
                            });
                            this.hls.loadSource(this.browserPlayUrl);
                            this.hls.attachMedia(video);

                            this.hls.on(Hls.Events.MANIFEST_PARSED, (event, data) => {
                                const availableQualities = this.hls.levels.map((l) => l.height);
                                availableQualities.sort((a, b) => b - a);
                                
                                const defQual = selectedQuality && availableQualities.includes(selectedQuality) ? selectedQuality : availableQualities[0];
                                defaultOptions.quality = {
                                    default: defQual,
                                    options: availableQualities,
                                    forced: true,
                                    onChange: (e) => this.updateQuality(e)
                                };
                                
                                initPlayer();
                                if (defQual) this.updateQuality(defQual);
                                this.player.play();
                            });
                        } else {
                            initPlayer();
                            video.src = this.browserPlayUrl;
                            video.addEventListener('loadedmetadata', () => {
                                this.player.play();
                            });
                        }
                    });
                },
                updateQuality(newQuality) {
                    if (this.hls) {
                        this.hls.levels.forEach((level, levelIndex) => {
                            if (level.height === newQuality) {
                                this.hls.currentLevel = levelIndex;
                            }
                        });
                    }
                }
            }
        });
    