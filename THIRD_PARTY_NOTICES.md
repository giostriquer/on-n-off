# Third-party notices

## Codenotch

The design of on-n-off's macOS side notch follows Codenotch 1.0 by @hivinz_
(bundle `com.vinz.codenotch`, published at
[vinzdg.github.io/usage-notch](https://vinzdg.github.io/usage-notch/)): the
notch silhouette that flares into the screen edge,
one ring cell per provider with the percentage beneath, the hover popover with
quota windows and live sessions, the "show on hover" pill, the four edge
options, and the shape of the Integrations settings. The design was studied
from the running application on 2026-09-01 and re-measured by hand; no
Codenotch source code, binaries, assets, or text are included in on-n-off, and
the Swift, Rust, and TypeScript implementations are on-n-off's own.

## T3 Code

Portions of on-n-off's Usage analytics are derived from
[T3 Code](https://github.com/pingdotgg/t3code), including transcript parsing,
usage aggregation, pricing, scan caching, data merging, and display formatting.
The port changed the implementation language and adapted the code to on-n-off's
Tauri architecture.

The source was taken from the repository state at commit
[`db1507e986591ae8e82f8fa1e173a9013309c64e`](https://github.com/pingdotgg/t3code/tree/db1507e986591ae8e82f8fa1e173a9013309c64e).
Relevant upstream files include:

- [`apps/server/src/usage/usageTranscripts.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/apps/server/src/usage/usageTranscripts.ts)
- [`apps/server/src/usage/usageAggregation.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/apps/server/src/usage/usageAggregation.ts)
- [`apps/server/src/usage/usagePricing.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/apps/server/src/usage/usagePricing.ts)
- [`apps/server/src/usage/usageScanCache.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/apps/server/src/usage/usageScanCache.ts)
- [`apps/server/src/usage/usageTranscriptReader.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/apps/server/src/usage/usageTranscriptReader.ts)
- [`apps/server/src/usage/UsageService.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/apps/server/src/usage/UsageService.ts)
- [`packages/shared/src/usageMerge.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/packages/shared/src/usageMerge.ts)
- [`packages/shared/src/usageFormat.ts`](https://github.com/pingdotgg/t3code/blob/db1507e986591ae8e82f8fa1e173a9013309c64e/packages/shared/src/usageFormat.ts)

T3 Code is distributed under the MIT License:

> MIT License
>
> Copyright (c) 2026 T3 Tools Inc.
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.
