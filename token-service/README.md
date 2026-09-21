# token-service

Holds the Atlassian OAuth client secret so `jira-tui` can log in without shipping credentials in the binary.

The TUI still catches the browser redirect at `http://127.0.0.1:8787/callback`. This service only exchanges the authorization code, refreshes tokens, and revokes them.

Point `jira-tui` at a non-default bind address with `JIRA_TUI_TOKEN_SERVICE`.

## Run

```bash
export ATLASSIAN_CLIENT_ID=...
export ATLASSIAN_CLIENT_SECRET=...
cargo run -p token-service
```

Listens on `http://127.0.0.1:8788` by default.

| Variable | Default |
| --- | --- |
| `ATLASSIAN_CLIENT_ID` | required |
| `ATLASSIAN_CLIENT_SECRET` | required |
| `OAUTH_REDIRECT_URI` | `http://127.0.0.1:8787/callback` |
| `BIND_ADDR` | `127.0.0.1:8788` locally; `0.0.0.0:$PORT` when `PORT` is set (Cloud Run) |

Leave `OAUTH_REDIRECT_URI` as the loopback URL even when this process is hosted. The TUI still catches the browser redirect.

The redirect URI must match the TUI callback and the Atlassian app setting character-for-character.

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | liveness |
| `GET` | `/v1/config` | `client_id` and `redirect_uri` for the TUI |
| `POST` | `/v1/oauth/exchange` | `{ "code", "code_verifier" }` |
| `POST` | `/v1/oauth/refresh` | `{ "refresh_token" }` |
| `POST` | `/v1/oauth/revoke` | `{ "token" }` |

## Atlassian app

1. Create an OAuth 2.0 (3LO) app in the [developer console](https://developer.atlassian.com/console/myapps/).
2. Set the callback URL to `http://127.0.0.1:8787/callback`.
3. Enable classic scopes `read:jira-work` and `write:jira-work`.
4. Under **Distribution**, enable sharing so other users can consent. Privacy policy: [PRIVACY.md](../PRIVACY.md). Use a URL that returns HTTP 200 with no redirect (GitHub Pages is the reliable host; a blob URL often 301s).

## Docker

Build from this directory:

```bash
docker build -t token-service .
docker run --rm -p 8080:8080 \
  -e ATLASSIAN_CLIENT_ID \
  -e ATLASSIAN_CLIENT_SECRET \
  token-service
```

The image listens on `0.0.0.0:8080` via `PORT`. Point the TUI at it with `JIRA_TUI_TOKEN_SERVICE=http://127.0.0.1:8080`.

## Cloud Run

The OAuth callback stays on the user's machine. Cloud Run only serves `/v1/config`, exchange, refresh, and revoke. Deploy unauthenticated: the TUI is a native client and cannot do Google IAP.

```bash
PROJECT=your-project
REGION=us-central1
REPO=jira-tui
IMAGE="${REGION}-docker.pkg.dev/${PROJECT}/${REPO}/token-service:latest"

gcloud config set project "$PROJECT"
gcloud services enable \
  run.googleapis.com \
  artifactregistry.googleapis.com \
  cloudbuild.googleapis.com \
  secretmanager.googleapis.com
gcloud artifacts repositories create "$REPO" \
  --repository-format=docker --location="$REGION" \
  --description="jira-tui images" || true

gcloud builds submit token-service \
  --config token-service/cloudbuild.yaml \
  --substitutions=_IMAGE="$IMAGE"

printf '%s' "$ATLASSIAN_CLIENT_ID" | gcloud secrets create atlassian-client-id --data-file=- \
  || printf '%s' "$ATLASSIAN_CLIENT_ID" | gcloud secrets versions add atlassian-client-id --data-file=-
printf '%s' "$ATLASSIAN_CLIENT_SECRET" | gcloud secrets create atlassian-client-secret --data-file=- \
  || printf '%s' "$ATLASSIAN_CLIENT_SECRET" | gcloud secrets versions add atlassian-client-secret --data-file=-

gcloud run deploy jira-tui-tokens \
  --image "$IMAGE" \
  --region "$REGION" \
  --allow-unauthenticated \
  --port 8080 \
  --cpu 1 --memory 256Mi \
  --max-instances 3 \
  --timeout 30 \
  --set-secrets=ATLASSIAN_CLIENT_ID=atlassian-client-id:latest,ATLASSIAN_CLIENT_SECRET=atlassian-client-secret:latest
```

`--set-secrets` grants the Cloud Run service account Secret Manager access. Then:

```bash
export JIRA_TUI_TOKEN_SERVICE="$(gcloud run services describe jira-tui-tokens \
  --region "$REGION" --format='value(status.url)')"
cargo run --release
```

Do not change the Atlassian callback URL. Optional next steps: a custom domain, Cloud Armor / rate limits on the POST routes, and baking `JIRA_TUI_TOKEN_SERVICE` into TUI releases.
