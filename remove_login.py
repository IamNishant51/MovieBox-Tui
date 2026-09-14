import re

with open('src/bin/index.html', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Remove the Login Modal HTML completely
modal_pattern = r'<div v-if="showLoginModal".*?<!-- Details Modal -->'
content = re.sub(modal_pattern, '<!-- Details Modal -->', content, flags=re.DOTALL)

# 2. Remove the setCategory login check
cat_check = r'if \(this\.loginRequired && \(cat === \'My List\' \|\| cat === \'History\'\) && !this\.user\) \{\s*this\.showLoginModal = true;\s*return;\s*\}'
content = re.sub(cat_check, '', content, flags=re.DOTALL)

# 3. Modify authFailed to never set showLoginModal
auth_failed_pattern = r'if \(this\.loginRequired\) \{\s*this\.user = null;\s*this\.showLoginModal = true;\s*\} else \{\s*this\.showToast\(detail \|\| \'Server blocked the request \?\?\? restart it with: cargo run --bin server\', true\);\s*\}'
content = re.sub(auth_failed_pattern, "this.showToast('Authentication disabled. Please refresh the page.', false);", content, flags=re.DOTALL)

# 4. Remove the Sign In button from navbar
sign_in_pattern = r'<button v-else-if="!user" @click="showLoginModal = true".*?</button>'
content = re.sub(sign_in_pattern, '', content, flags=re.DOTALL)

with open('src/bin/index.html', 'w', encoding='utf-8') as f:
    f.write(content)
print("Removed Google Login Modal Completely")
