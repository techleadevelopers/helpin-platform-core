param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [int]$Vus = 50,
  [string]$Duration = "2m"
)

$ErrorActionPreference = "Stop"
$env:BASE_URL = $BaseUrl
$env:K6_VUS = "$Vus"
$env:K6_DURATION = $Duration

k6 run "$PSScriptRoot\..\k6\http-rescue-feed.js"
