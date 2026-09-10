export const SESSION_COOKIE_NAME = "trading_desk_session";
export const SESSION_DURATION_SECONDS = 7 * 24 * 60 * 60; // 7 days

function getSecretKey(): string {
  return (
    process.env.SESSION_SECRET ||
    process.env.ENGINE_SECRET_KEY ||
    "antigravity-dev-session-secret-2026"
  );
}

function toBase64Url(buffer: ArrayBuffer | Uint8Array): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(base64url: string): Uint8Array {
  let base64 = base64url.replace(/-/g, "+").replace(/_/g, "/");
  while (base64.length % 4) {
    base64 += "=";
  }
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

async function getCryptoKey(): Promise<CryptoKey> {
  const secret = getSecretKey();
  const encoder = new TextEncoder();
  return await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"]
  );
}

export async function createSessionToken(userId = "trader_admin"): Promise<string> {
  const payload = {
    sub: userId,
    exp: Date.now() + SESSION_DURATION_SECONDS * 1000,
  };
  const encoder = new TextEncoder();
  const payloadStr = JSON.stringify(payload);
  const payloadB64 = toBase64Url(encoder.encode(payloadStr));

  const key = await getCryptoKey();
  const signature = await crypto.subtle.sign("HMAC", key, encoder.encode(payloadB64));
  const sigB64 = toBase64Url(signature);

  return `${payloadB64}.${sigB64}`;
}

export async function verifySessionToken(token: string | undefined): Promise<boolean> {
  if (!token) return false;
  const parts = token.split(".");
  if (parts.length !== 2) return false;

  const [payloadB64, sigB64] = parts;
  try {
    const key = await getCryptoKey();
    const encoder = new TextEncoder();
    const sigBytes = fromBase64Url(sigB64);

    const isValid = await crypto.subtle.verify("HMAC", key, sigBytes as BufferSource, encoder.encode(payloadB64));
    if (!isValid) return false;

    const decoder = new TextDecoder();
    const payloadBytes = fromBase64Url(payloadB64);
    const payloadStr = decoder.decode(payloadBytes);
    const payload = JSON.parse(payloadStr);

    if (!payload.exp || Date.now() > payload.exp) {
      return false;
    }
    return true;
  } catch {
    return false;
  }
}
