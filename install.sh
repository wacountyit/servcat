#!/bin/bash
set -e # exit immediately if any command fails, rather than plowing ahead

echo "=== ServCat Installer ==="

# --- Check prerequisites ---
if ! command -v docker &> /dev/null; then
        echo "Docker is not installed. Install Docker first: https://docs.docker.com/engine/install/"
        exit 1
fi

if ! docker compose version &> /dev/null; then
        echo "Docker Compose plugin is not available. Install it before continuing."
        exit 1
fi

if ! command -v openssl &> /dev/null; then
        echo "openssl is required to generate secrets. Install it before continuing."
        exit 1
fi

# --- Generate .env if it doesn't exist ---
if [ -f .env ]; then
        echo ".env already exists -- skipping secret generation (existing install detected)."
else
        echo "Generating secrets..."
        DB_ROOT_PASS=$(openssl rand -hex 24)
        DB_PASS=$(openssl rand -hex 24)
        JWT_SIGNING_SECRET=$(openssl rand -base64 48)

        cat > .env << EOF
DB_ROOT_PASS=${DB_ROOT_PASS}
DB_NAME=servcat
DB_USER=servcat_user
DB_PASS=${DB_PASS}
DATABASE_URL=mysql://servcat_user:${DB_PASS}@127.0.0.1:3306/servcat

JWT_SIGNING_SECRET=${JWT_SIGNING_SECRET}
EOF

        chmod 600 .env # restrict readability to the owning user only
        echo ".env generated with strong random secrets."
fi

# --- Host port ---
# The container always listens on 8080 internally; only the host-side
# mapping is configurable (docker-compose.yml's APP_PORT), in case
# something else on this machine already has 8080.
port_in_use() {
        (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

if grep -q "^APP_PORT=" .env 2>/dev/null; then
        app_port=$(grep -E '^APP_PORT=' .env | cut -d '=' -f2-)
        echo "Host port already recorded in .env ($app_port) -- skipping prompt."
else
        default_port=8080
        while port_in_use "$default_port"; do
                default_port=$((default_port + 1))
        done
        if [ "$default_port" != "8080" ]; then
                echo ""
                echo "Port 8080 already looks taken on this machine."
        fi
        echo ""
        read -rp "Host port to expose ServCat on [default: ${default_port}]: " app_port
        app_port=${app_port:-$default_port}
        echo "APP_PORT=${app_port}" >> .env
fi

# --- Organization branding ---
if grep -q "^APP_NAME=" .env 2>/dev/null; then
        echo "Organization name already recorded in .env -- skipping prompt."
else
        echo ""
        read -rp "Organization name to show alongside ServCat (e.g. \"Your Organization\"), or leave blank for just \"ServCat\": " org_name
        if [ -n "$org_name" ]; then
                echo "APP_NAME=${org_name} ServCat" >> .env
        else
                echo "APP_NAME=ServCat" >> .env
        fi
        echo "This only seeds the name on first startup -- an admin can rename it later from within the app."
fi

# --- Reverse proxy setup ---
# HTTPS is only a hard requirement once Entra ID SSO is in the picture (its
# redirect URI must be HTTPS) and is good practice for carrying login
# tokens generally -- but nothing in the app itself enforces it, so local
# testing over plain HTTP is fine.
if grep -q "^COMPOSE_PROFILES=" .env 2>/dev/null; then
        echo "Reverse proxy choice already recorded in .env -- skipping prompt."
else
        echo ""
        echo "Do you already have a reverse proxy in front of this host"
        echo "(NGINX Proxy Manager, Traefik, etc.)?"
        echo "  1) Yes -- I'll point my own proxy at it"
        echo "  2) No -- set one up for me (Caddy, automatic TLS)"
        echo "  3) No -- I'm just testing locally for now, skip HTTPS"
        read -rp "Choice [1/2/3]: " proxy_choice

        if [ "$proxy_choice" = "3" ]; then
                echo "COMPOSE_PROFILES=" >> .env
                public_base_url="http://localhost:${app_port}"
                echo ""
                echo "Skipping HTTPS/reverse proxy. Once containers are up, the API is"
                echo "reachable directly at ${public_base_url} (try ${public_base_url}/health)."
                echo "Set up a reverse proxy (or Caddy) and Entra ID SSO before using this"
                echo "for anything beyond local testing."
        elif [ "$proxy_choice" = "2" ]; then
                echo ""
                read -rp "Domain name pointing at this server (leave blank if none / using a bare IP): " domain

                if [ -n "$domain" ]; then
                        cat > Caddyfile << EOF
${domain} {
    reverse_proxy app:8080
}
EOF

                        echo "Caddyfile written for domain '${domain}' -- Caddy will obtain a real cert automatically via Let's Encrypt."
                        public_base_url="https://${domain}"
                else
                        cat > Caddyfile << EOF
:443 {
    tls internal
    reverse_proxy app:8080
}
EOF

                        echo "Caddyfile written for IP-only access -- Caddy will use a self-signed cert."
                        echo "Your browser will show a certificate warning the first time; that's expected without a domain."
                        public_base_url="https://<this-host-ip>"
                fi

                echo "COMPOSE_PROFILES=caddy" >> .env
        else
                echo "COMPOSE_PROFILES=" >> .env
                echo ""
                read -rp "Public HTTPS URL your existing reverse proxy serves ServCat at (e.g. https://servcat.example.com): " public_base_url
                echo ""
                echo "Point your existing reverse proxy's upstream at:"
                echo "  http://<this-host-ip>:${app_port}"
                echo "and make sure it terminates HTTPS on the browser-facing side."
        fi
fi

# --- Frontend / CORS ---
# The React/Tauri frontend is a separate app that calls this API; without
# its origin allow-listed here, the browser will block every request.
if grep -q "^CORS_ALLOWED_ORIGINS=" .env 2>/dev/null; then
        echo "CORS origins already recorded in .env -- skipping prompt."
else
        echo ""
        echo "Where will the ServCat frontend be served from? This is whatever origin"
        echo "the browser loads the web app from -- it can be the same domain as above"
        echo "(most common if the frontend is served by the same reverse proxy), a"
        echo "different subdomain, or left blank for local development only. This"
        echo "only matters for browser requests -- it's fine to leave the default if"
        echo "you're just testing the API directly (curl, Postman, etc.)."
        read -rp "Frontend origin(s), comma-separated [default: ${public_base_url:-http://localhost:5173}]: " cors_origins
        cors_origins=${cors_origins:-${public_base_url:-http://localhost:5173}}
        echo "CORS_ALLOWED_ORIGINS=${cors_origins}" >> .env
fi

# --- Bootstrap administrator ---
# Needed either way: with local sign-up off (the default) and no users
# provisioned yet, this is the only account that can log in and create
# others (or pre-provision users by email ahead of their first SSO login).
if grep -q "^BOOTSTRAP_ADMIN_EMAIL=" .env 2>/dev/null; then
        echo "Bootstrap admin already recorded in .env -- skipping prompt."
else
        echo ""
        echo "Create a bootstrap administrator account (local email/password,"
        echo "independent of SSO) to log in with the first time and set everything"
        echo "else up. Rotate or disable it once real admin accounts exist."
        read -rp "Bootstrap admin email: " admin_email
        bootstrap_password=$(openssl rand -base64 18)
        cat >> .env << EOF
BOOTSTRAP_ADMIN_EMAIL=${admin_email}
BOOTSTRAP_ADMIN_PASSWORD=${bootstrap_password}
EOF
        echo ""
        echo "Bootstrap admin password (save this now -- it will not be shown again):"
        echo "  ${bootstrap_password}"
fi

# --- Local account creation toggle ---
if grep -q "^ALLOW_LOCAL_SIGNUP=" .env 2>/dev/null; then
        echo "Local sign-up setting already recorded in .env -- skipping prompt."
else
        echo ""
        echo "By default, only the bootstrap admin and SSO sign-ins can create"
        echo "accounts -- local email/password self-registration stays disabled and"
        echo "hidden in the frontend. Most orgs on SSO should leave this off."
        read -rp "Allow local email/password self-registration too? [y/N]: " allow_signup
        if [[ "$allow_signup" =~ ^[Yy]$ ]]; then
                echo "ALLOW_LOCAL_SIGNUP=true" >> .env
        else
                echo "ALLOW_LOCAL_SIGNUP=false" >> .env
        fi
        echo "This only seeds the setting on first startup -- an admin can toggle it later from within the app."
fi

# --- Microsoft Entra ID (SSO) setup ---
if grep -q "^AZURE_TENANT_ID=" .env 2>/dev/null; then
        echo "Microsoft Entra ID settings already recorded in .env -- skipping prompt."
else
        echo ""
        echo "ServCat can sign users in with your organization's Microsoft Entra ID"
        echo "(Azure AD) tenant. This is optional -- leave everything blank to skip"
        echo "SSO for now and rely on the bootstrap admin/local accounts only; you"
        echo "can add these to .env and restart later."
        echo ""
        echo "If you're setting this up now, register an app in the Azure Portal"
        echo "under 'App registrations' first, and add a redirect URI there"
        echo "matching what you enter below."
        echo ""
        read -rp "Entra ID Tenant ID (blank to skip SSO): " azure_tenant_id

        if [ -n "$azure_tenant_id" ]; then
                read -rp "Application (client) ID: " azure_client_id
                read -rsp "Client secret (*Value* -- input hidden): " azure_client_secret
                echo ""
                read -rp "Redirect URI [default: ${public_base_url:-https://<this-host>}/auth/sso/callback]: " azure_redirect_uri
                azure_redirect_uri=${azure_redirect_uri:-${public_base_url:-https://<this-host>}/auth/sso/callback}
                read -rp "Frontend URL to send users back to after sign-in [default: ${cors_origins:-${public_base_url}}/auth/sso/complete]: " sso_frontend_redirect_url
                sso_frontend_redirect_url=${sso_frontend_redirect_url:-${cors_origins:-${public_base_url}}/auth/sso/complete}

                cat >> .env << EOF
AZURE_TENANT_ID=${azure_tenant_id}
AZURE_CLIENT_ID=${azure_client_id}
AZURE_CLIENT_SECRET=${azure_client_secret}
AZURE_REDIRECT_URI=${azure_redirect_uri}
SSO_FRONTEND_REDIRECT_URL=${sso_frontend_redirect_url}
EOF
                echo "Microsoft Entra ID settings saved to .env."
                echo "NOTE: make sure the Azure app registration's redirect URI matches"
                echo "      AZURE_REDIRECT_URI exactly: ${azure_redirect_uri}"
        else
                cat >> .env << EOF
AZURE_TENANT_ID=
AZURE_CLIENT_ID=
AZURE_CLIENT_SECRET=
AZURE_REDIRECT_URI=
SSO_FRONTEND_REDIRECT_URL=
EOF
                echo "Skipping SSO setup for now."
        fi
fi

# --- Build and start ---
echo ""
echo "Building and starting containers..."
docker compose up -d --build

echo ""
echo "=== Done ==="

if grep -q "^COMPOSE_PROFILES=caddy" .env 2>/dev/null; then
        echo "ServCat is starting up behind Caddy. Give it a few seconds, then visit:"
        echo "  https://<this-host-ip-or-domain>"
else
        echo "ServCat is starting up. Give it a few seconds, then visit it through"
        echo "your reverse proxy's HTTPS URL."
fi
echo ""
echo "This repo is the API only -- point your reverse proxy's frontend origin"
echo "at wherever the ServCat web app is deployed, and confirm it matches"
echo "CORS_ALLOWED_ORIGINS in .env."
echo ""
echo "Installation complete."
