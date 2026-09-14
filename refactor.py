import re

with open('src/bin/index.html', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Remove "Play in Browser" button entirely
play_regex = re.compile(r'<button @click="promptQuality\(stream, activeMirror\(stream, idx\), idx, \'play\'\)".*?<i class="fas fa-play mr-1"></i> Play\s*</button>', re.DOTALL)
content = play_regex.sub('', content)

# 2. Change 'VLC/Play in App' button to be the primary 'Play' button
vlc_btn_pattern = r'<button @click="openVlcModal\(stream, activeMirror\(stream, idx\)\)" class="btn-touch flex-1 sm:flex-none bg-gray-700 hover:bg-gray-600 px-4 py-2 rounded text-sm font-bold transition-all text-center shadow-md">\s*<i :class="isMobile \? \'fas fa-external-link-alt mr-1\' : \'fas fa-desktop mr-1\'"></i> \{\{ isMobile \? \'Play in App\' : \'VLC\' \}\}\s*</button>'
new_vlc_btn = r'''<button @click="openVlcModal(stream, activeMirror(stream, idx), idx)" class="btn-touch flex-1 sm:flex-none bg-white text-black hover:bg-gray-200 px-4 py-2 rounded text-sm font-bold transition-all text-center shadow-md">
                                              <i :class="isMobile ? 'fas fa-external-link-alt mr-1' : 'fas fa-play mr-1'"></i> {{ isMobile ? 'Play in App' : 'Play' }}
                                          </button>'''
content = re.sub(vlc_btn_pattern, new_vlc_btn, content, flags=re.DOTALL)

# 3. Add Quality Selector in the stream card
mirrors_pattern = r'(<div v-if="stream.mirrors && stream.mirrors.length > 1" class="flex gap-1.5 overflow-x-auto scrollbar-hide pb-2 mb-1">.*?</div>)'
quality_html = r'''\1
                                      <div v-if="streamQualities[idx] && streamQualities[idx].length > 1" class="flex gap-1.5 overflow-x-auto scrollbar-hide pb-2 mb-1 items-center">
                                          <span class="text-xs text-gray-400 font-bold mr-1">Quality:</span>
                                          <button v-for="q in streamQualities[idx]" :key="q" @click="$set(selectedQuality, idx, q)" :class="['btn-touch flex-none chip !text-[.65rem] !py-1.5', (selectedQuality[idx] || streamQualities[idx][0]) === q ? '!bg-white !text-black !border-white' : 'hover:border-gray-300']">{{ q === 'Auto' ? 'Auto' : q + 'p' }}</button>
                                      </div>'''
content = re.sub(mirrors_pattern, quality_html, content, flags=re.DOTALL)

# 4. Remove Web Player DOM completely
web_player_pattern = r'<!-- Web Player -->.*?</div>\s*</div>\s*<div class="text-xs'
content = re.sub(web_player_pattern, '</div>\n\n                    <div class="text-xs', content, flags=re.DOTALL)

# 5. Remove Quality Picker Modal completely
quality_modal_pattern = r'<!-- Quality Picker Modal -->.*?</div>\s*</div>'
content = re.sub(quality_modal_pattern, '', content, flags=re.DOTALL)

# 6. Add state variables
content = content.replace('streams: [],', 'streams: [],\n                streamQualities: [],\n                selectedQuality: [],')

# 7. Update fetchStreams to pre-fetch qualities
fetch_streams_pattern = r'(this\.streams = data \|\| \[\];)'
fetch_qualities_logic = r'''
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
                              });'''
content = re.sub(fetch_streams_pattern, lambda m: m.group(1) + fetch_qualities_logic, content)


# 8. Update executeVlcLaunch and openVlcModal to use selectedQuality
content = content.replace('async openVlcModal(stream, mirror) {', 'async openVlcModal(stream, mirror, idx) {')
content = content.replace('this.pendingVlcMirror = mirror;', 'this.pendingVlcMirror = mirror;\n                    this.pendingVlcIdx = idx;\n                    this.executeVlcLaunch();\n                    return;')

# In executeVlcLaunch, append quality to sidecar if available
exec_vlc_pattern = r'let sidecarFetchUrl = `/api/sidecar\?url=\$\{encodeURIComponent\(this\.pendingVlcMirror\.resolver_url\)\}&headers=\$\{encodeURIComponent\(headersStr\)\}`;'
new_exec_vlc = r'''let sidecarFetchUrl = `/api/sidecar?url=${encodeURIComponent(this.pendingVlcMirror.resolver_url)}&headers=${encodeURIComponent(headersStr)}`;
                            const sq = this.selectedQuality[this.pendingVlcIdx];
                            if (sq && sq !== 'Auto') sidecarFetchUrl += `&quality=${sq}`;'''
content = re.sub(exec_vlc_pattern, new_exec_vlc, content)

# 9. Update download prompt
content = content.replace('promptQuality(stream, activeMirror(stream, idx), idx, \'download\')', 'downloadStream(stream, activeMirror(stream, idx), idx, (selectedQuality[idx] === \'Auto\' ? null : selectedQuality[idx]))')

# Clean up vue logic that is no longer used (promptQuality, confirmQuality, playInBrowser)
content = re.sub(r'promptQuality\(stream, mirror, idx, type\) \{.*?\},\s*confirmQuality\(q\) \{.*?\},\s*async playInBrowser\(stream, mirror, selectedQuality\) \{.*?\},\s*updateQuality\(newQuality\) \{.*?\},', '', content, flags=re.DOTALL)

with open('src/bin/index.html', 'w', encoding='utf-8') as f:
    f.write(content)
print("Updated index.html")
