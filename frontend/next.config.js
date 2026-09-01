/** @type {import('next').NextConfig} */
const nextConfig = {
  async rewrites() {
    const apiInternalUrl = process.env.API_INTERNAL_URL ?? "http://127.0.0.1:8080";
    return [{ source: "/api/:path*", destination: `${apiInternalUrl}/api/:path*` }];
  },
};

module.exports = nextConfig;
