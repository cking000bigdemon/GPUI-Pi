<#
.SYNOPSIS
从 Pi 会话 JSONL 中只读导出用户消息里的图片附件，供视觉子代理按绝对路径读取。

.DESCRIPTION
默认检查最后一条用户消息，并在该消息没有图片时失败，避免静默复用旧截图。
可以用 -EntryId 精确选择一条或多条消息，或用 -AllSince 导出指定时间后的所有图片消息。
必须传入 -ExpectedImageCount；实际数量不一致时脚本失败。
证据写入仓库根目录的 .pi/visual-review/<round>/evidence/，同时生成包含哈希和绝对路径的 manifest。
脚本以共享只读方式读取活跃会话，绝不修改会话文件；同名证据存在但内容不一致时会停止。

.EXAMPLE
.\scripts\export-visual-evidence.ps1 -Round round-12 -ExpectedImageCount 3

.EXAMPLE
.\scripts\export-visual-evidence.ps1 -Round round-12 -EntryId 7df36a85,89abcdef -ExpectedImageCount 5

.EXAMPLE
.\scripts\export-visual-evidence.ps1 -Round round-12 -AllSince -Since "2026-08-19T02:40:00Z" -ExpectedImageCount 5
#>
[CmdletBinding(DefaultParameterSetName = "Latest")]
param(
    [string]$SessionFile = $env:PI_SESSION_FILE,

    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._-]*$')]
    [string]$Round = "adhoc",

    [Parameter(ParameterSetName = "Entries", Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string[]]$EntryId,

    [Parameter(ParameterSetName = "AllSince", Mandatory = $true)]
    [switch]$AllSince,

    [Parameter(ParameterSetName = "AllSince", Mandatory = $true)]
    [DateTimeOffset]$Since,

    [ValidateRange(0, 1000)]
    [int]$ExpectedImageCount = 0
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)

$Root = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($SessionFile)) {
    throw "No session file was provided. Set PI_SESSION_FILE or pass -SessionFile."
}
if ($ExpectedImageCount -lt 1) {
    throw "Pass -ExpectedImageCount with the number of screenshots in the visual-review request."
}

$SessionPath = (Resolve-Path -LiteralPath $SessionFile -ErrorAction Stop).Path
$OutputPath = [System.IO.Path]::GetFullPath(
    (Join-Path $Root ".pi\visual-review\$Round\evidence")
)
[System.IO.Directory]::CreateDirectory($OutputPath) | Out-Null

function Read-SharedUtf8Text([string]$Path) {
    $Share = [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete
    $Stream = New-Object System.IO.FileStream(
        $Path,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        $Share
    )
    try {
        $Encoding = New-Object System.Text.UTF8Encoding($false, $true)
        $Reader = New-Object System.IO.StreamReader($Stream, $Encoding, $true)
        try {
            return $Reader.ReadToEnd()
        }
        finally {
            $Reader.Dispose()
        }
    }
    finally {
        $Stream.Dispose()
    }
}

$SessionText = Read-SharedUtf8Text $SessionPath
$Lines = $SessionText -split "\r?\n"
$AllEntriesById = @{}
$UserMessages = New-Object System.Collections.Generic.List[object]
$UserMessagesById = @{}
$LineNumber = 0

foreach ($Line in $Lines) {
    $LineNumber++
    if ([string]::IsNullOrWhiteSpace($Line)) { continue }

    try {
        $Entry = $Line | ConvertFrom-Json
    }
    catch {
        throw "Invalid or incomplete JSON in session file at line $LineNumber. The session may still be writing; retry the export."
    }

    $CurrentEntryId = [string]$Entry.id
    if (-not [string]::IsNullOrWhiteSpace($CurrentEntryId)) {
        $AllEntriesById[$CurrentEntryId] = [pscustomobject]@{
            type = [string]$Entry.type
            role = [string]$Entry.message.role
        }
    }

    if ($Entry.type -ne "message" -or $Entry.message.role -ne "user") {
        continue
    }

    $Content = @()
    if ($null -ne $Entry.message.content -and -not ($Entry.message.content -is [string])) {
        $Content = @($Entry.message.content)
    }
    $Images = @($Content | Where-Object {
        $_.type -eq "image" -and -not [string]::IsNullOrWhiteSpace([string]$_.data)
    })

    try {
        $MessageTime = [DateTimeOffset]::Parse(
            [string]$Entry.timestamp,
            [System.Globalization.CultureInfo]::InvariantCulture,
            [System.Globalization.DateTimeStyles]::RoundtripKind
        )
    }
    catch {
        throw "User message '$CurrentEntryId' has an invalid timestamp."
    }

    $Record = [pscustomobject]@{
        Id            = $CurrentEntryId
        Timestamp     = [string]$Entry.timestamp
        TimestampValue = $MessageTime
        Images        = @($Images)
    }
    $UserMessages.Add($Record)
    if (-not [string]::IsNullOrWhiteSpace($CurrentEntryId)) {
        $UserMessagesById[$CurrentEntryId] = $Record
    }
}

if ($UserMessages.Count -eq 0) {
    throw "The session does not contain any user messages."
}

$SelectedMessages = New-Object System.Collections.Generic.List[object]

switch ($PSCmdlet.ParameterSetName) {
    "Latest" {
        $Latest = $UserMessages[$UserMessages.Count - 1]
        if ($Latest.Images.Count -eq 0) {
            throw "The latest user message '$($Latest.Id)' at $($Latest.Timestamp) has no image attachments. Refusing to reuse older screenshots; attach the evidence in the latest message or select exact entries."
        }
        $SelectedMessages.Add($Latest)
    }

    "Entries" {
        $SeenIds = @{}
        foreach ($RequestedId in $EntryId) {
            if ([string]::IsNullOrWhiteSpace($RequestedId)) {
                throw "-EntryId contains an empty value."
            }
            if ($SeenIds.ContainsKey($RequestedId)) {
                throw "-EntryId contains duplicate value '$RequestedId'."
            }
            $SeenIds[$RequestedId] = $true

            if (-not $UserMessagesById.ContainsKey($RequestedId)) {
                if ($AllEntriesById.ContainsKey($RequestedId)) {
                    $Found = $AllEntriesById[$RequestedId]
                    throw "Session entry '$RequestedId' exists but is not a user message (type='$($Found.type)', role='$($Found.role)')."
                }
                throw "Session entry '$RequestedId' was not found."
            }

            $Message = $UserMessagesById[$RequestedId]
            if ($Message.Images.Count -eq 0) {
                throw "User message '$RequestedId' does not contain image attachments."
            }
            $SelectedMessages.Add($Message)
        }
    }

    "AllSince" {
        foreach ($Message in $UserMessages) {
            if ($Message.TimestampValue -ge $Since -and $Message.Images.Count -gt 0) {
                $SelectedMessages.Add($Message)
            }
        }
        if ($SelectedMessages.Count -eq 0) {
            throw "No user image messages were found at or after $($Since.ToString('o'))."
        }
    }

    default {
        throw "Unsupported selection mode '$($PSCmdlet.ParameterSetName)'."
    }
}

$ActualImageCount = 0
foreach ($Message in $SelectedMessages) {
    $ActualImageCount += $Message.Images.Count
}
if ($ActualImageCount -ne $ExpectedImageCount) {
    $SelectedIds = @($SelectedMessages | ForEach-Object { $_.Id }) -join ", "
    throw "Expected $ExpectedImageCount image(s), but selected $ActualImageCount from entry/entries: $SelectedIds. Refusing incomplete or stale evidence."
}

function Get-ByteHash([byte[]]$Bytes) {
    $Hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $HashBytes = $Hasher.ComputeHash($Bytes)
        return (($HashBytes | ForEach-Object { $_.ToString("x2") }) -join "")
    }
    finally {
        $Hasher.Dispose()
    }
}

# 证据不可静默覆盖：同一路径只能复用完全相同的内容。
function Write-ImmutableBytes([string]$Path, [byte[]]$Bytes, [string]$ExpectedHash) {
    if (Test-Path -LiteralPath $Path) {
        $ExistingHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($ExistingHash -ne $ExpectedHash) {
            throw "Evidence path already exists with different content: $Path"
        }
        return
    }

    $TempPath = Join-Path (Split-Path -Parent $Path) (".tmp-" + [guid]::NewGuid().ToString("N"))
    try {
        [System.IO.File]::WriteAllBytes($TempPath, $Bytes)
        [System.IO.File]::Move($TempPath, $Path)
    }
    finally {
        if (Test-Path -LiteralPath $TempPath) {
            Remove-Item -LiteralPath $TempPath -Force
        }
    }
}

$Extensions = @{
    "image/png"  = "png"
    "image/jpeg" = "jpg"
    "image/jpg"  = "jpg"
    "image/webp" = "webp"
    "image/gif"  = "gif"
    "image/bmp"  = "bmp"
}

$ImageRecords = New-Object System.Collections.Generic.List[object]
$SelectedEntryRecords = New-Object System.Collections.Generic.List[object]
$GlobalImageIndex = 0

foreach ($Message in $SelectedMessages) {
    $SelectedEntryRecords.Add([pscustomobject]@{
        entryId          = $Message.Id
        messageTimestamp = $Message.Timestamp
        imageCount       = $Message.Images.Count
    })

    $AttachmentIndex = 0
    foreach ($Image in @($Message.Images)) {
        $AttachmentIndex++
        $GlobalImageIndex++
        $MimeType = ([string]$Image.mimeType).ToLowerInvariant()
        if (-not $Extensions.ContainsKey($MimeType)) {
            throw "Unsupported image MIME type '$MimeType' in entry '$($Message.Id)'."
        }

        $Base64 = [string]$Image.data
        if ($Base64 -match '^data:[^;]+;base64,(.*)$') {
            $Base64 = $Matches[1]
        }
        $Base64 = $Base64 -replace '\s', ''

        try {
            [byte[]]$Bytes = [System.Convert]::FromBase64String($Base64)
        }
        catch {
            throw "Invalid base64 image data in entry '$($Message.Id)', attachment $AttachmentIndex."
        }
        if ($Bytes.Length -eq 0) {
            throw "Empty image data in entry '$($Message.Id)', attachment $AttachmentIndex."
        }

        $Hash = Get-ByteHash $Bytes
        $SafeEntryId = [regex]::Replace($Message.Id, '[^A-Za-z0-9_-]', '_')
        $FileName = "{0}-{1:D2}.{2}" -f $SafeEntryId, $AttachmentIndex, $Extensions[$MimeType]
        $ImagePath = Join-Path $OutputPath $FileName
        Write-ImmutableBytes $ImagePath $Bytes $Hash

        $ImageRecords.Add([pscustomobject]@{
            index            = $GlobalImageIndex
            entryId          = $Message.Id
            messageTimestamp = $Message.Timestamp
            attachmentIndex  = $AttachmentIndex
            path             = [System.IO.Path]::GetFullPath($ImagePath)
            mimeType         = $MimeType
            bytes            = $Bytes.Length
            sha256           = $Hash
        })
    }
}

$Utf8 = New-Object System.Text.UTF8Encoding($false)
$SelectionIds = @($SelectedMessages | ForEach-Object { $_.Id }) -join "`n"
$SinceMaterial = if ($PSCmdlet.ParameterSetName -eq "AllSince") { $Since.ToString("o") } else { "" }
$SelectionMaterial = "$SessionPath`n$Round`n$($PSCmdlet.ParameterSetName)`n$SinceMaterial`n$ExpectedImageCount`n$SelectionIds"
$SelectionHash = (Get-ByteHash $Utf8.GetBytes($SelectionMaterial)).Substring(0, 16)
$ManifestPath = Join-Path $OutputPath ("manifest-$SelectionHash.json")
$Manifest = [pscustomobject]@{
    version            = 1
    sessionFile        = $SessionPath
    round              = $Round
    selectionMode      = $PSCmdlet.ParameterSetName
    since              = if ($PSCmdlet.ParameterSetName -eq "AllSince") { $Since.ToString("o") } else { $null }
    expectedImageCount = $ExpectedImageCount
    actualImageCount   = $ActualImageCount
    manifestPath       = [System.IO.Path]::GetFullPath($ManifestPath)
    selectedEntries    = $SelectedEntryRecords.ToArray()
    images             = $ImageRecords.ToArray()
}

$ManifestJson = ($Manifest | ConvertTo-Json -Depth 8) + [Environment]::NewLine
$ManifestBytes = $Utf8.GetBytes($ManifestJson)
$ManifestHash = Get-ByteHash $ManifestBytes
Write-ImmutableBytes $ManifestPath $ManifestBytes $ManifestHash

$Manifest | ConvertTo-Json -Depth 8
exit 0
