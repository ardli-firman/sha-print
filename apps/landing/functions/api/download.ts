import { packageIds, type PackageId } from "../../src/releases";
interface Env {
  DOWNLOAD_METRICS?: D1Database;
}
const headers = {
  "Cache-Control": "no-store",
  "X-Content-Type-Options": "nosniff",
};
const reply = (status: number) => new Response(null, { status, headers });

export const onRequest: PagesFunction<Env> = async ({ request, env }) => {
  if (request.method !== "POST")
    return new Response(null, {
      status: 405,
      headers: { ...headers, Allow: "POST" },
    });
  // CORS, origin and payload allowlists keep arbitrary values out of the database.
  if (request.headers.get("Origin") !== new URL(request.url).origin)
    return reply(403);
  if (request.headers.get("Sec-Fetch-Site") === "cross-site") return reply(403);
  if (
    request.headers.get("DNT") === "1" ||
    request.headers.get("Sec-GPC") === "1"
  )
    return reply(204);
  if (!request.headers.get("Content-Type")?.startsWith("application/json"))
    return reply(415);
  if (Number(request.headers.get("Content-Length")) > 128) return reply(413);
  let id: unknown;
  try {
    // Stop reading after a small fixed limit, including chunked requests.
    const reader = request.body?.getReader();
    if (!reader) return reply(400);
    let bytes = 0;
    const chunks: Uint8Array[] = [];
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      if (bytes > 128) {
        await reader.cancel();
        return reply(413);
      }
      chunks.push(value);
    }
    const buffer = new Uint8Array(bytes);
    let offset = 0;
    for (const chunk of chunks) {
      buffer.set(chunk, offset);
      offset += chunk.length;
    }
    const payload = JSON.parse(new TextDecoder().decode(buffer));
    if (
      !payload ||
      typeof payload !== "object" ||
      Array.isArray(payload) ||
      Object.keys(payload).length !== 1
    )
      return reply(400);
    id = payload.package;
  } catch {
    return reply(400);
  }
  if (!packageIds.includes(id as PackageId)) return reply(400);
  if (!env.DOWNLOAD_METRICS) return reply(503);
  try {
    await env.DOWNLOAD_METRICS.prepare(
      "INSERT INTO download_counts (day, package, clicks) VALUES (?, ?, 1) ON CONFLICT(day, package) DO UPDATE SET clicks = clicks + 1",
    )
      .bind(new Date().toISOString().slice(0, 10), id)
      .run();
    return reply(204);
  } catch {
    // Do not log visitor input. The package link remains usable independently.
    return reply(503);
  }
};
