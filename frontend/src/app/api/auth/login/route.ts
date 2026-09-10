import { NextRequest, NextResponse } from "next/server";
import { createSessionToken, SESSION_COOKIE_NAME, SESSION_DURATION_SECONDS } from "@/lib/auth";

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const pin = (body.pin || body.password || "").trim();

    const expectedPin = process.env.DASHBOARD_PIN || process.env.ADMIN_PIN;

    // If a DASHBOARD_PIN is configured, enforce strict verification
    if (expectedPin) {
      if (pin !== expectedPin.trim()) {
        return NextResponse.json(
          { error: "Invalid Access PIN" },
          { status: 401 }
        );
      }
    }

    const token = await createSessionToken("trader_admin");

    const response = NextResponse.json({
      success: true,
      message: "Session authenticated",
    });

    response.cookies.set({
      name: SESSION_COOKIE_NAME,
      value: token,
      httpOnly: true,
      secure: process.env.NODE_ENV === "production",
      sameSite: "strict",
      path: "/",
      maxAge: SESSION_DURATION_SECONDS,
    });

    return response;
  } catch (error) {
    return NextResponse.json(
      { error: "Authentication processing failed" },
      { status: 500 }
    );
  }
}
