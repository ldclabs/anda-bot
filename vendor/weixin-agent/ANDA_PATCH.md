# Local transport patch

Source: https://github.com/ldclabs/weixin-agent-sdk-rs/tree/12096bac4d5401884573fb3e7991825f67ccc6f5
License: MIT (see LICENSE).

This pinned copy adds an optional reqwest Client to WeixinConfigBuilder and
uses that client for API, QR login, downloads and CDN uploads. Anda supplies
its configured client so proxy settings and connection pools are shared.
No protocol behavior is changed. Remove this copy when upstream exposes the
same injection point. The legacy CDN upload function remains compatible.
