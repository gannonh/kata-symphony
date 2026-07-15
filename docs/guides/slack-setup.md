---
type: Guide
title: Slack channel setup
description: Connect a Slack workspace to Kata Agents for channel read/respond.
tags: [slack, channels, setup]
timestamp: 2026-07-15T20:00:00Z
---

# Slack Channel Setup

Connect a Slack workspace to Kata Agents so the agent can read and respond to messages in your Slack channels.

## Prerequisites

- A Slack workspace where you have permission to install apps
- Kata Agents desktop app installed with at least one workspace configured

## Part 1: Create a Slack App

1. Go to [api.slack.com/apps](https://api.slack.com/apps) and sign in.
2. Click **Create New App**, then select **From scratch**.
3. Enter an app name (e.g., "Kata Agent") and select your Slack workspace.
4. Click **Create App**.

> Classic Slack apps are no longer supported. Use the standard app creation flow above.

## Part 2: Add Bot Token Scopes

1. In the left sidebar, click **OAuth & Permissions**.
2. Scroll to **Scopes**, then under **Bot Token Scopes** add:
   - `channels:history` -- read messages in public channels
   - `groups:history` -- read messages in private channels
   - `chat:write` -- send messages
   - `users:read` -- resolve user display names
3. Scroll up and click **Install to Workspace**, then authorize the app.
4. Copy the **Bot User OAuth Token** (starts with `xoxb-`). You will paste this into Kata Agents later.

## Part 3: Enable Socket Mode (for slash commands)

Skip this section if you do not need slash commands.

1. In the left sidebar, click **Socket Mode**.
2. Toggle **Enable Socket Mode** to on.
3. When prompted, create an app-level token:
   - Name it (e.g., "kata-socket")
   - Add the `connections:write` scope
   - Click **Generate**
4. Copy the app-level token (starts with `xapp-`). You will paste this into Kata Agents later.
5. In the left sidebar, click **Slash Commands**.
6. Click **Create New Command**.
7. Fill in the command details:
   - **Command:** `/kata` (or your preferred name)
   - **Short Description:** "Ask the Kata agent"
   - **Usage Hint:** `[your question]`
8. Click **Save**. Socket Mode handles delivery, so you do not need a Request URL.

## Part 4: Invite the Bot to Channels

The bot can only read and post in channels where it has been added.

1. Open the Slack channel you want the bot to monitor.
2. Invite the bot using one of these methods:
   - Type `/invite @YourBotName` in the channel
   - Right-click the channel name, select **View channel details**, go to **Integrations**, then click **Add apps**
3. Repeat for each channel.

## Part 5: Find Channel IDs

Kata Agents needs the Slack channel ID (not the channel name).

1. In Slack, right-click the channel name and select **View channel details**.
2. Scroll to the bottom of the details panel. The **Channel ID** appears there (e.g., `C01ABCDEF`).
3. Copy the ID.

Alternative: open the channel in a browser. The channel ID is the segment after `/archives/` in the URL (e.g., `https://app.slack.com/client/T.../C01ABCDEF`).

## Part 6: Configure in Kata Agents

1. Open **Settings** and navigate to **Channels**.
2. Click the **+** button to open the new channel form.
3. Select **Slack** as the adapter type.
4. Fill in the form fields:
   - **Name** -- a short identifier (e.g., `my-team-alerts`)
   - **Bot Token** -- paste the `xoxb-` token from Part 2
   - **App-Level Token (optional)** -- paste the `xapp-` token from Part 3 (required for slash commands)
   - **Channel IDs** -- paste one or more channel IDs from Part 5, comma-separated (e.g., `C01234567, C07654321`)
   - **Poll Interval (ms)** -- how often to check for new messages, in milliseconds (default: `10000`)
   - **Trigger Patterns** -- optional comma-separated regex patterns that filter which messages activate the agent (e.g., `help.*,urgent.*`)
5. Click **Save Channel**.
6. Start the daemon: click **Start** in the Daemon section at the top of the Channels page, or use the tray icon.

The channel toggle in the Configured Channels list controls whether the channel is active. The daemon must also be running for message processing to work.

## Troubleshooting

**Bot does not respond to messages**

- Confirm the bot is invited to the channel (Part 4).
- Confirm the channel ID is correct (Part 5).
- Confirm the daemon is running (the Daemon status indicator on the Channels page shows "running").
- If you added trigger patterns, verify the message content matches at least one pattern.

**Slash commands do not work**

- Confirm Socket Mode is enabled in the Slack app settings (Part 3, step 2).
- Confirm the app-level token (`xapp-`) is entered in the **App-Level Token** field.
- Confirm the slash command is created in the Slack app settings (Part 3, steps 5-8).
- After changing Socket Mode settings, restart the daemon.

**Permission errors from the Slack API**

- Verify all four required scopes are added (Part 2, step 2).
- After adding or changing scopes, you must reinstall the app: go to **OAuth & Permissions** and click **Reinstall to Workspace**.
- Copy the new bot token after reinstalling (the token may change).

**Messages appear twice**

- The adapter filters out messages from the bot itself. If you still see duplicates, check that trigger patterns are not overly broad.
- Verify you have not configured the same channel ID in multiple Kata Agents channels.
