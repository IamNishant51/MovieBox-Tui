import re

with open('src/bin/index.html', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Update the Search Bar Input UI
search_bar_pattern = r'<input v-model="searchQuery" @input="onSuggestInput".*?<button @click="toggleSearch" aria-label="Search".*?</button>'
new_search_bar = r'''
                        <div class="relative flex items-center transition-all duration-300" :class="searchActive ? 'w-64 sm:w-72 md:w-80 border border-white bg-black/80' : 'w-[44px] bg-transparent border-none'">
                            <button @click="toggleSearch" aria-label="Search" class="text-white hover:text-gray-300 min-h-[44px] min-w-[44px] flex items-center justify-center transition-all z-10 flex-none bg-transparent outline-none">
                                <i class="fas fa-search"></i>
                            </button>
                            <input ref="searchInput" v-model="searchQuery" @input="onSuggestInput" @keydown.esc="clearSearch" type="text" placeholder="Titles, people, genres" autocomplete="off" aria-label="Search titles" :class="['bg-transparent text-white py-1.5 text-sm md:text-base focus:outline-none transition-all min-h-[36px] w-full pr-10', searchActive ? 'opacity-100' : 'opacity-0 hidden']" />
                            <button v-if="searchActive && searchQuery" @click="clearSearch" class="absolute right-0 text-white hover:text-gray-300 min-h-[44px] min-w-[44px] flex items-center justify-center z-10">
                                <i class="fas fa-times"></i>
                            </button>
                        </div>
'''
content = re.sub(search_bar_pattern, new_search_bar, content, flags=re.DOTALL)

# 2. Remove the suggest panel completely
suggest_panel_pattern = r'<div v-if="showSuggest && \(suggestResults\.length \|\| suggestStrings\.length\)".*?</div>\s*</div>'
content = re.sub(suggest_panel_pattern, '</div>', content, flags=re.DOTALL)

# 3. Add a "Search" category view replacement in the main content area
main_content_pattern = r'(<!-- Trending/Movies/Series Grid -->\s*<div v-else-if="\[\'Trending\', \'Movies\', \'Series\'\]\.includes\(category\)" class="px-4 md:px-12 py-8 pt-20">)'
new_search_view = r'''
            <!-- Search Results Grid -->
            <div v-if="category === 'Search'" class="px-4 md:px-12 py-8 pt-28 min-h-screen">
                <h2 class="text-xl md:text-2xl font-bold text-gray-400 mb-6">Explore titles related to: <span class="text-white">{{ searchQuery }}</span></h2>
                <div v-if="allResults.length === 0 && !loadingSearch" class="text-center text-gray-500 mt-20">No matching results found.</div>
                <div class="grid grid-cols-2 min-[400px]:grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 gap-2 md:gap-4">
                    <div v-if="loadingSearch" v-for="i in 18" :key="'skel-s-'+i" class="w-full aspect-[2/3] bg-[#262626] rounded-lg skeleton-shimmer"></div>
                    <div v-else v-for="(item, idx) in allResults" :key="item.id.value + '-' + idx" @click="fetchDetails(item)" class="movie-card relative rounded-lg overflow-hidden cursor-pointer group bg-[#111]">
                        <img v-if="item.poster_url" :src="getTmdbImage(item.poster_url, 'w500')" class="w-full aspect-[2/3] object-cover bg-gray-800" loading="lazy" />
                        <div v-else class="w-full aspect-[2/3] flex items-center justify-center bg-gray-800 text-gray-500 text-3xl"><i class="fas fa-image"></i></div>
                        <div class="absolute inset-0 bg-gradient-to-t from-black via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-2 md:p-3">
                            <h3 class="text-xs md:text-sm font-bold text-white leading-tight drop-shadow-md">{{ item.title }}</h3>
                            <div class="flex items-center gap-1 md:gap-2 text-[0.6rem] md:text-xs text-gray-300 mt-1">
                                <span class="text-green-500 font-bold" v-if="item.year">{{ item.year }}</span>
                                <span v-if="item.media_type" class="uppercase">{{ item.media_type === 'movie' ? 'Movie' : 'TV' }}</span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
            \1
'''
content = re.sub(main_content_pattern, new_search_view, content, count=1)

# 4. Modify Vue methods to handle Search correctly
js_replacements = {
    'this.searchActive = !this.searchActive;': 'this.searchActive = !this.searchActive; if (this.searchActive) { this.$nextTick(() => this.$refs.searchInput.focus()); } else { this.clearSearch(); }',
    'const q = (this.searchQuery || \'\').trim();\n                    if (q.length < 2) { this.suggestResults = []; this.suggestStrings = []; this.showSuggest = false; return; }\n                    this.suggestTimer = setTimeout(() => this.fetchSuggest(q), 300);': 'const q = (this.searchQuery || \'\').trim();\n                    if (q.length === 0) { this.clearSearch(); return; }\n                    if (this.category !== \'Search\') this.prevCategory = this.category;\n                    this.category = \'Search\';\n                    this.allResults = [];\n                    this.loadingSearch = true;\n                    this.suggestTimer = setTimeout(() => this.executeSearch(q), 500);',
    'async fetchSuggest(q) {': 'async executeSearch(q) {\n                    try {\n                        const res = await fetch(`/api/search?q=${encodeURIComponent(q)}`);\n                        const data = await res.json();\n                        this.allResults = data.results || [];\n                    } catch (e) { this.gridError = e.message; }\n                    finally { this.loadingSearch = false; }\n                },\n                clearSearch() {\n                    this.searchQuery = "";\n                    this.searchActive = false;\n                    if (this.category === \'Search\') this.category = this.prevCategory || \'Trending\';\n                },\n                async fetchSuggest(q) {'
}

for k, v in js_replacements.items():
    content = content.replace(k, v)

# Update state variables in data()
content = content.replace("searchQuery: '',", "searchQuery: '', prevCategory: null, loadingSearch: false,")

with open('src/bin/index.html', 'w', encoding='utf-8') as f:
    f.write(content)
print("Applied Netflix Search UI Refactor")
