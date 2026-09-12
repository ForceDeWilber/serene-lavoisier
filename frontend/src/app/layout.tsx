import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Multi-Venue Crypto Algorithmic Terminal",
  description: "Real-time control plane for Revolut X and Kraken Pro algorithmic trading bots",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <body className="min-h-screen bg-[#0d1117] text-[#e6edf3] antialiased selection:bg-[#388bfd]/30 selection:text-white">
        {children}
      </body>
    </html>
  );
}
