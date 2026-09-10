import { NextRequest, NextResponse } from "next/server";
import { verifySessionToken, SESSION_COOKIE_NAME } from "@/lib/auth";

export async function middleware(request: NextRequest) {
  const pinConfigured = Boolean(process.env.DASHBOARD_PIN || process.env.ADMIN_PIN);

  // If no PIN is set, permit all access (frictionless local dev)
  if (!pinConfigured) {
    return NextResponse.next();
  }

  const { pathname } = request.nextUrl;

  // Allow static assets, favicon, and auth routes
  if (
    pathname.startsWith("/login") ||
    pathname.startsWith("/api/auth") ||
    pathname.startsWith("/_next") ||
    pathname.includes(".")
  ) {
    return NextResponse.next();
  }

  // Only gate /dashboard
  if (pathname.startsWith("/dashboard")) {
    const token = request.cookies.get(SESSION_COOKIE_NAME)?.value;
    const isValid = await verifySessionToken(token);

    if (!isValid) {
      const loginUrl = new URL("/login", request.url);
      loginUrl.searchParams.set("returnUrl", pathname);
      return NextResponse.redirect(loginUrl);
    }
  }

  return NextResponse.next();
}

export const config = {
  matcher: ["/dashboard/:path*", "/login"],
};
