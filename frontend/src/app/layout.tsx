import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "RustySERRF",
  description: "SERRF normalization for metabolomics data",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
