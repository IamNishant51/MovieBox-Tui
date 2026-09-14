import re

with open('src/bin/index.html', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Update Typography (Netflix Sans -> Inter)
content = content.replace("font-family: 'Netflix Sans', 'Helvetica Neue', Helvetica, Arial, sans-serif;", "font-family: 'Inter', 'Helvetica Neue', Helvetica, Arial, sans-serif;")
# Add Inter font link
head_pattern = r'<link rel="dns-prefetch" href="https://cdn.plyr.io">'
new_head = r'<link rel="dns-prefetch" href="https://cdn.plyr.io">\n    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800;900&display=swap" rel="stylesheet">'
content = re.sub(head_pattern, new_head, content)

# Update font-bold in Hero title to font-extrabold tracking-tight
hero_title_pattern = r'<h1 class="text-3xl md:text-5xl lg:text-7xl font-bold text-white mb-2 md:mb-4 drop-shadow-2xl hero-fade-enter-active"'
new_hero_title = r'<h1 class="text-3xl md:text-5xl lg:text-7xl font-extrabold tracking-tight text-white mb-2 md:mb-4 drop-shadow-2xl hero-fade-enter-active"'
content = re.sub(hero_title_pattern, new_hero_title, content)

# 2. Update Grid Responsiveness
content = content.replace('grid-cols-3 sm:grid-cols-4', 'grid-cols-2 min-[400px]:grid-cols-3 sm:grid-cols-4')

# 3. Update Horizontal Swiping Rows (w-40 to w-[35vw] md:w-40)
content = content.replace('w-40 sm:w-48', 'w-[35vw] md:w-40 sm:w-48')

# 4. Premium Animations (Delay hover)
hover_css_pattern = r'\.movie-card:hover \{ transform: scale\(1\.05\); z-index: 10; box-shadow: 0 10px 20px rgba\(0,0,0,0\.8\); \}'
new_hover_css = r'.movie-card { transition: transform 0.4s cubic-bezier(0.25, 1, 0.5, 1), box-shadow 0.4s ease; }\n        .movie-card:hover { transform: scale(1.05); z-index: 10; box-shadow: 0 10px 20px rgba(0,0,0,0.8); transition-delay: 150ms; }'
content = re.sub(r'\.movie-card \{ transition: transform 0\.3s ease, box-shadow 0\.3s ease; \}\s*\.movie-card:hover \{ transform: scale\(1\.05\); z-index: 10; box-shadow: 0 10px 20px rgba\(0,0,0,0\.8\); \}', new_hover_css, content)

# Ken Burns effect for Hero
ken_burns = r'''
        @keyframes kenBurns {
            0% { transform: scale(1.0); }
            100% { transform: scale(1.05); }
        }
        .ken-burns { animation: kenBurns 15s ease-out forwards; }
'''
content = content.replace('</style>', ken_burns + '</style>')
content = content.replace(':src="getTmdbImage(activeHero.poster, \'w1280\')"', ':src="getTmdbImage(activeHero.poster, \'w1280\')" class="ken-burns"')

with open('src/bin/index.html', 'w', encoding='utf-8') as f:
    f.write(content)
print("Applied Typography and Responsiveness Fixes")
