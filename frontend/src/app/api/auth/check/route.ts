import { NextRequest, NextResponse } from "next/server";
import { verifySessionToken, SESSION_COOKIE_NAME } from "@/lib/auth";

export async function GET(req: NextRequest) {
  const pinConfigured = Boolean(process.env.DASHBOARD_PIN || process.env.ADMIN_PIN);
  
  if (!pinConfigured) {
    return NextResponse.json({
      authenticated: true,
      authRequired: false,
    });
  }

  const token = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  const isValid = await verifySessionToken(token);

  return NextResponse.json({
    authenticated: isValid,
    authRequired: true,
  });
}
