# Changelog

## [0.2.1](https://github.com/elisiumm/nlm/compare/v0.2.0...v0.2.1) (2026-04-23)


### Bug Fixes

* **notion:** improve error detail, render child pages and tables inline ([1452998](https://github.com/elisiumm/nlm/commit/1452998e913ec0388ee54b07cddd8fbe98a33beb))

## [0.2.0](https://github.com/elisiumm/nlm/compare/v0.1.1...v0.2.0) (2026-04-15)


### Features

* **adapters:** implement Notion sync adapter (Phase 3) ([d297060](https://github.com/elisiumm/nlm/commit/d297060cc36a34ba0ab5d2d403454af9a1e25e1b))
* **cli:** add --debug flag to generate, fetch, correct ([f5bf57e](https://github.com/elisiumm/nlm/commit/f5bf57e95f7c9d08e055562f0fb664af6fb6e683))
* **pptx:** implement brand charter extraction (Phase 4) ([9284825](https://github.com/elisiumm/nlm/commit/9284825470cb3446fa11d9a112112c4cc8cbef31))
* **upload:** upload binary sources (images, PDFs) via resumable protocol ([977f9d0](https://github.com/elisiumm/nlm/commit/977f9d059a89bee50101609187316140784e96e5))


### Bug Fixes

* **client:** resolve notebook id from correct response fields ([cda8da0](https://github.com/elisiumm/nlm/commit/cda8da00ec3cf40b8f242e2be26e708383996565))
* **generate:** pass completed source IDs to CREATE_ARTIFACT ([b7479e1](https://github.com/elisiumm/nlm/commit/b7479e1e29b08498808a87ac7eddc1fc1da9fcbd))
* **rpc:** surface batchexecute raw item on null result ([c62063b](https://github.com/elisiumm/nlm/commit/c62063b6694a5f36b9125b4142c2b9c64b5f5fc8))
* **upload:** add Content-Type + Accept headers on resumable flow ([a519e8a](https://github.com/elisiumm/nlm/commit/a519e8a8fcb9db3ea9fcdcfb9a35d0338810640d))

## [0.1.1](https://github.com/elisiumm/nlm/compare/v0.1.0...v0.1.1) (2026-04-02)


### Bug Fixes

* **artifact:** fix source ID extraction, pass source IDs to generation, and route by default_artifact ([328c800](https://github.com/elisiumm/nlm/commit/328c8006a42be37eb6047518faf4b7289694c6ab))
* **run:** address copilot review — guard skip-upload, filter ready sources, fix comments ([f22157d](https://github.com/elisiumm/nlm/commit/f22157d4242b61030fd1bb069db418e81b6d6c7a))

## 0.1.0 (2026-03-27)


### Features

* **cli:** initial public release of nlm rust cli ([dbfecf7](https://github.com/elisiumm/nlm/commit/dbfecf7dd628c0500c9279e90e8f93346756c7e2))
* **correct:** implement phase 3b targeted slide correction via REVISE_SLIDE RPC ([82153c9](https://github.com/elisiumm/nlm/commit/82153c9672b8a1e365c9867f2eb960a6f4161645))


### Bug Fixes

* **ci:** resolve pre-existing fmt and clippy violations ([5efcb9e](https://github.com/elisiumm/nlm/commit/5efcb9e2df11fb53b0422bf8cd9e059e08c6b1bb))
