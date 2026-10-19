import { describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { onRequest } from "../functions/api/download";
const schema = await Bun.file(
  new URL("../migrations/0001_download_counts.sql", import.meta.url),
).text();
const origin = "https://sha-print.pages.dev";
function request(body: string, headers: Record<string, string> = {}) {
  return new Request(`${origin}/api/download`, {
    method: "POST",
    headers: { Origin: origin, "Content-Type": "application/json", ...headers },
    body,
  });
}
async function run(request: Request, db?: D1Database) {
  return onRequest({ request, env: { DOWNLOAD_METRICS: db } } as Parameters<
    typeof onRequest
  >[0]);
}
function sqliteD1() {
  const sqlite = new Database(":memory:");
  sqlite.exec(schema);
  const db = {
    prepare: (sql: string) => ({
      bind: (...values: unknown[]) => ({
        run: async () => sqlite.query(sql).run(...(values as string[])),
      }),
    }),
  } as unknown as D1Database;
  return { sqlite, db };
}
describe("download counter", () => {
  test("aggregates by UTC day and package without storing visitor fields", async () => {
    const { sqlite, db } = sqliteD1();
    try {
      for (const id of ["windows-exe", "windows-exe", "linux-deb"])
        expect(
          (await run(request(JSON.stringify({ package: id })), db)).status,
        ).toBe(204);
      const rows = sqlite
        .query("SELECT * FROM download_counts ORDER BY package")
        .all();
      expect(rows).toEqual([
        {
          day: new Date().toISOString().slice(0, 10),
          package: "linux-deb",
          clicks: 1,
        },
        {
          day: new Date().toISOString().slice(0, 10),
          package: "windows-exe",
          clicks: 2,
        },
      ]);
    } finally {
      sqlite.close();
    }
  });
  test("rejects cross-origin events before touching storage", async () => {
    expect(
      (
        await run(
          request('{"package":"windows-exe"}', {
            Origin: "https://example.com",
          }),
        )
      ).status,
    ).toBe(403);
  });
  test("rejects requests without an origin", async () => {
    const req = request('{"package":"windows-exe"}');
    req.headers.delete("Origin");
    expect((await run(req)).status).toBe(403);
  });
  test("honors DNT and GPC without requiring storage", async () => {
    for (const headers of [{ DNT: "1" }, { "Sec-GPC": "1" }] as Record<
      string,
      string
    >[])
      expect(
        (await run(request('{"package":"windows-exe"}', headers))).status,
      ).toBe(204);
  });
  test("rejects unknown packages, extra fields, malformed JSON and arrays", async () => {
    for (const body of [
      '{"package":"unknown"}',
      '{"package":"windows-exe","extra":true}',
      "not-json",
      "[]",
      "null",
    ])
      expect((await run(request(body))).status).toBe(400);
  });
  test("bounds request bodies even without Content-Length", async () => {
    expect((await run(request("x".repeat(129)))).status).toBe(413);
  });
  test("only accepts JSON POSTs", async () => {
    expect((await run(new Request(`${origin}/api/download`))).status).toBe(405);
    expect(
      (await run(request("{}", { "Content-Type": "text/plain" }))).status,
    ).toBe(415);
  });
  test("reports absent or failing storage without leaking its error", async () => {
    expect((await run(request('{"package":"windows-exe"}'))).status).toBe(503);
    const broken = {
      prepare: () => {
        throw new Error("database unavailable");
      },
    } as unknown as D1Database;
    const response = await run(request('{"package":"windows-exe"}'), broken);
    expect(response.status).toBe(503);
    expect(await response.text()).toBe("");
  });
});
