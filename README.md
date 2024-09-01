# polli.live

The [polli.live](https://polli.live) server allows creating interactive polls for presentations. It's a fairly simple server that leaves most user-interface decisions to the talk author. It's mainly intended to be used with [revealjs](https://revealjs.com/) presentations but the server is independent of that.

### Design Principles

- The server should not have to change to accomondate different kinds of polls.
- No persistent storage or data-base required.
- Only simple `http` requests for improved compatibility and reliability.
- Load balancing should be possible.
- Sessions can survive restarts of the server, presenter and audience hardware.

### API

- `GET` `/`
  - Default home page which allows manually entering the session id.
  - Usually it's expected that the audience scans a QR code or so instead though.
- `POST` `/new`
  - Responds with `{session: <id>, token: <token>}`.
  - Initializes a new session and is tied to a specific token.
  - It's possible to reuse a previous session if possible and desired.
    - For that pass the following json as request body: `{session: <desired-id>, token: <desired-token>}`.
    - It may be that the session is used by someone else with a different token now. In that case, a new session is created instead.
- `POST` `/page?session=<id>`
  - Requires `Authorization: Bearer <token>` http header.
  - Request body should be an html document that is delivered to the audience.
  - This also deletes all responses that were still stored for the previous page.
- `POST` `/respond?session=<id>&user=<id>`
  - Sends a poll response from an audience member.
  - The response replaces any previous response by that user.
- `GET` `/responses?session=<id>&start=<start>`
  - Responds with `{next_start: <id>, responses_by_user: {<user>: <response>}}`
  - Retrieves all responses starting at the given start id.
  - The `start` should be zero at first. After that it should be the retrieved `next_start` value.
- `GET` `/page?session=<id>`
  - Retrieves page stored for that session or responds with a 404 status code.
  - The server injects some code into the page to provide the `polli_live.respond(data_str)` function that can be used to send data back.
