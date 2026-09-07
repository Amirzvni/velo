# Velo Privacy Policy

Last updated: 2026-09-07

## Short version

Velo does not collect, transmit, or sell any of your data. There is no
analytics, no telemetry, and no account. Everything stays on your computer.

## What the browser extension does

The Velo browser extension talks to the Velo application running on your own
computer, at `http://127.0.0.1`. Nothing is sent anywhere else.

When you download a file through Velo, the extension passes the following to
the local application so the download can succeed:

- the download link you clicked or selected
- the address of the page you were on (the referer)
- your browser's user agent string
- cookies for that specific link, when the site requires them to serve the file

These are the same details your browser would have sent to download the file
itself. They are used only to fetch your file and are not stored beyond the
download record kept on your machine.

## What the application stores

Velo keeps a local database in your user folder containing your download
history: file names, links, sizes and progress. You can delete any entry from
inside the app, and uninstalling Velo with its data removes the database.

Downloaded files are saved where you choose, by default in your Downloads
folder.

## What Velo never does

- No data leaves your computer, except the requests needed to download the
  files you asked for, sent directly to those file servers.
- No analytics, crash reporting, advertising, or tracking of any kind.
- No account, sign in, or user identifier.

## Permissions the extension asks for, and why

- `downloads` — to notice a download starting in the browser and hand it to
  Velo instead.
- `storage` — to remember the local port and access token for the app on your
  machine.
- `contextMenus` — to add the "Download with Velo" right click entries.
- `scripting` and `activeTab` — to show the link picker panel on the page you
  are looking at, when you ask for it.
- `host_permissions` for `http://127.0.0.1/*` — to reach the Velo application
  on your own computer. No other host is contacted by the extension.

## Source code

Velo is open source. You can read exactly what it does at
https://github.com/Amirzvni/velo

## Contact

Questions about this policy: open an issue at
https://github.com/Amirzvni/velo/issues
