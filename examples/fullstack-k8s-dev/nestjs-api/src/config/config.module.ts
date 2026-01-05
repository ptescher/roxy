import { Module } from "@nestjs/common";
import { HttpModule } from "@nestjs/axios";
import { ConfigModule, ConfigService } from "@nestjs/config";
import { ConfigServiceClient } from "./config.service";
import { Agent as HttpAgent } from "http";
import { Agent as HttpsAgent } from "https";

// Dynamic import for proxy agent support
let HttpProxyAgent: any;
let HttpsProxyAgent: any;

try {
  // These are optional dependencies - if not available, proxy won't work
  HttpProxyAgent = require("http-proxy-agent").HttpProxyAgent;
  HttpsProxyAgent = require("https-proxy-agent").HttpsProxyAgent;
} catch {
  // Proxy agents not available
}

function createProxyAgents(configService: ConfigService): {
  httpAgent?: HttpAgent;
  httpsAgent?: HttpsAgent;
} {
  const httpProxy = process.env.HTTP_PROXY || process.env.http_proxy;
  const httpsProxy = process.env.HTTPS_PROXY || process.env.https_proxy;

  const agents: { httpAgent?: HttpAgent; httpsAgent?: HttpsAgent } = {};

  if (httpProxy && HttpProxyAgent) {
    agents.httpAgent = new HttpProxyAgent(httpProxy);
    console.log(`[ConfigModule] HTTP proxy agent configured: ${httpProxy}`);
  }

  if (httpsProxy && HttpsProxyAgent) {
    agents.httpsAgent = new HttpsProxyAgent(httpsProxy);
    console.log(`[ConfigModule] HTTPS proxy agent configured: ${httpsProxy}`);
  } else if (httpProxy && HttpsProxyAgent) {
    // Fall back to HTTP_PROXY for HTTPS requests if HTTPS_PROXY not set
    agents.httpsAgent = new HttpsProxyAgent(httpProxy);
  }

  return agents;
}

@Module({
  imports: [
    HttpModule.registerAsync({
      imports: [ConfigModule],
      inject: [ConfigService],
      useFactory: (configService: ConfigService) => {
        const baseURL = configService.get<string>(
          "CONFIG_SERVICE_URL",
          "http://config-service.backend.svc.cluster.local:8080",
        );

        const proxyAgents = createProxyAgents(configService);

        console.log(`[ConfigModule] Config service URL: ${baseURL}`);
        if (proxyAgents.httpAgent || proxyAgents.httpsAgent) {
          console.log(
            "[ConfigModule] Proxy agents enabled for config-service requests",
          );
        }

        return {
          baseURL,
          timeout: 5000,
          headers: {
            "Content-Type": "application/json",
          },
          ...proxyAgents,
        };
      },
    }),
  ],
  providers: [ConfigServiceClient],
  exports: [ConfigServiceClient],
})
export class AppConfigModule {}
