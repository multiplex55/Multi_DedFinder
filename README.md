# eve-ded-route

`eve-ded-route` generates quiet high-sec DED route waypoint plans from public ESI activity data and prepared static/local SDE data. It can optionally push the generated waypoint list through ESI for a configured character.

## Safety and data-source boundary

This project stays within a strict data-source boundary:

- It uses public ESI activity data and static/local SDE-derived data for route generation.
- It may use authenticated ESI only for character location lookup and waypoint writes when you configure those features.
- It does **not** provide live anomaly detection, probe scanner parsing, EVE UI automation, in-client clicking, EVE client process interaction, or gameplay botting.

## Complete config example

Save a config similar to this as `config.toml` and adjust paths, character values, ESI app details, and route preferences for your environment.

```toml
[start]
# Use the authenticated character's current solar system as the route start.
# A CLI --start value overrides this source and uses the named system instead.
source = "character_location"
# Optional fallback if character-location lookup fails.
system = "Dodixie"
fallback_to_config_system = true

[character]
# Used for authenticated ESI character-location lookup and waypoint push.
id = 123456789
name = "Config Pilot"

[data]
# Directory containing prepared systems.csv/stargates.csv and, when name-based
# region features are used, optional regions.csv or regions.json.
sde_path = "./data/sde"

[route]
waypoint_count = 25
max_distance = 40
mode = "dense_quiet"
output = "text"
output_path = "./routes/route.txt"
json_path = "./routes/route.json"
push_waypoints = false
prefer_loop = true
trade_hub_radius = 3
route_history_enabled = true
route_history_path = "./routes/route-history.json"
route_history_last_route_only = true
ignore_malformed_route_history = false

[filter]
highsec_only = true
min_security_status = 0.45
max_distance_from_start = 40
max_jumps_last_hour = 80
max_npc_kills_last_hour = 250
max_ship_kills_last_hour = 25
max_pod_kills_last_hour = 10
activity_behavior = "hard_exclude"
# Trade hub behavior supports "hard_exclude", "soft_penalty", or "disabled".
# The radius is [route].trade_hub_radius; the soft amount is below.
trade_hub_behavior = "soft_penalty"
trade_hubs = ["Jita", "Amarr", "Dodixie", "Rens", "Hek"]
trade_hub_soft_penalty = 0.25

[avoid]
# System names are resolved from the loaded SDE systems data.
systems = ["Jita", "Perimeter", "Uedama", "Sivala", "Ahbazon"]
# Region names require regions.csv or regions.json in [data].sde_path.
regions = ["Pochven"]
# Numeric region IDs work without region-name files.
region_ids = [10000070]

[weights]
activity = 1.0
distance = 1.0
security = 1.0

[faction_space]
# behavior: "disabled", "hard_include", or "soft_bonus".
behavior = "soft_bonus"
preferred_factions = ["gallente"]
excluded_factions = ["triglavian"]
soft_bonus = 0.15
# exclude_behavior: "disabled", "candidate_only", or "hard_exclude".
exclude_behavior = "candidate_only"

[faction_space.factions.gallente]
# Region names require regions.csv or regions.json.
regions = ["Essence", "Sinq Laison"]
# Numeric IDs do not require region-name files.
region_ids = [10000064]

[faction_space.factions.triglavian]
regions = ["Pochven"]
region_ids = [10000070]

[esi]
client_id = "your-eve-application-client-id"
callback_url = "http://localhost:8080/callback"
# Include waypoint scope when pushing waypoints, and location scope when using
# [start].source = "character_location". If both features are enabled, include both.
scopes = [
  "esi-ui.write_waypoint.v1",
  "esi-location.read_location.v1",
]
activity_cache_minutes = 15
activity_cache_path = "./cache/activity.json"
allow_stale_activity_cache = false
```

## Commands

Generate a route using only config values:

```sh
eve-ded-route --config config.toml generate
```

Push an existing config route JSON using the character configured in `[character]`:

```sh
eve-ded-route --config config.toml push
```

By default, `push` reads `[route].json_path`. You can override it with `--json`:

```sh
eve-ded-route --config config.toml push --json ./routes/route.json
```

Generate and push in one config-driven run by setting `[route].push_waypoints = true`, including the required ESI scopes, and using `generate --yes` to skip confirmation prompts:

```toml
[route]
json_path = "./routes/route.json"
push_waypoints = true
```

```sh
eve-ded-route --config config.toml generate --yes
```

## Config and CLI precedence

Config files provide defaults. CLI flags override config values for a run:

- CLI flags override config values generally.
- `--json` overrides `[route].json_path` for JSON generation and route JSON selection during `push`.
- `--character-id` and `--character-name` override `[character].id` and `[character].name`.
- `--start` overrides `[start].source = "character_location"` by switching the start source to the named configured start system for that invocation.

## SDE region data

The SDE directory must contain prepared system and stargate data for route generation. Region-name data has narrower requirements:

- `regions.csv` or `regions.json` is optional for general route generation.
- `regions.csv` or `regions.json` is required when you use name-based region avoidance in `[avoid].regions`.
- `regions.csv` or `regions.json` is required when faction-space region names are configured under `[faction_space.factions.<name>].regions`.
- Numeric `[avoid].region_ids` and faction-space `region_ids` work without region-name files.
