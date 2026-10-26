# Release gate (F1 (d)): typing in the real window, on WebView2, as she types. The installed
# app is open on a kept vault (invented data, tests/fixtures/vaults); this opens it with the
# fixtures' password, opens the case, opens "הדבקת טקסט" for a material, and types 500 Hebrew
# characters one key at a time through Windows' own input (SendInput, Unicode), while the
# window's status checks run. The text field must then hold exactly what was typed: no key
# lost, doubled or out of order, and the window must have kept up.
#
# Then (L6, D-117) the way her keyboard really sends Hebrew: the window is switched to the
# Hebrew layout and the keys themselves are pressed (virtual-key and scan codes, not Unicode),
# so Windows' own layout, and the text input of WebView2 behind it, turn them into letters.
# This is the path on which typing stopped before F1(a).
#
# Runs in Windows PowerShell 5.1 (UI Automation ships with it). Reads the window only through
# UI Automation, the way a screen reader does; nothing in the app is switched on for the test.
param(
    [Parameter(Mandatory = $true)] [int] $ProcessId,
    [string] $Password = "כלב ירוק רץ מהר בגינה",
    [int] $Characters = 500
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Keys {
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    // As large as the union's largest member (MOUSEINPUT, 32 bytes on x64): SendInput refuses
    // any other INPUT size with error 87.
    [StructLayout(LayoutKind.Explicit)]
    struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad0; [FieldOffset(8)] public long pad1; [FieldOffset(16)] public long pad2; [FieldOffset(24)] public long pad3; }
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT { public uint type; public INPUTUNION u; }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);
    const uint KEYBOARD = 1, KEYUP = 0x2, UNICODE = 0x4;
    static void Send(ushort vk, ushort scan, uint flags) {
        var i = new INPUT[1];
        i[0].type = KEYBOARD;
        i[0].u.ki = new KEYBDINPUT { wVk = vk, wScan = scan, dwFlags = flags };
        if (SendInput(1, i, Marshal.SizeOf(typeof(INPUT))) != 1) throw new Exception("SendInput failed: " + Marshal.GetLastWin32Error());
    }
    public static void Char(char c) { Send(0, c, UNICODE); Send(0, c, UNICODE | KEYUP); }
    // A physical key, as a keyboard sends it: what letter it makes is the window's layout's choice.
    [DllImport("user32.dll")]
    static extern uint MapVirtualKey(uint code, uint mapType);
    public static void Key(ushort vk) {
        var scan = (ushort)MapVirtualKey(vk, 0);
        Send(vk, scan, 0);
        Send(vk, scan, KEYUP);
    }
    public static void Enter() { Send(0x0D, 0, 0); Send(0x0D, 0, KEYUP); }
    public static void Down() { Send(0x28, 0, 0); Send(0x28, 0, KEYUP); }
}
// The keyboard layout of the window that has the keyboard focus (in WebView2 a window of its
// browser process, not of the app's own), and a request to that window to switch it.
public static class Layout {
    [StructLayout(LayoutKind.Sequential)]
    struct RECT { public int l, t, r, b; }
    [StructLayout(LayoutKind.Sequential)]
    struct GUITHREADINFO { public int cbSize; public uint flags; public IntPtr active, focus, capture, menuOwner, moveSize, caret; public RECT caretRect; }
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint thread, ref GUITHREADINFO info);
    [DllImport("user32.dll")] static extern IntPtr GetKeyboardLayout(uint thread);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr LoadKeyboardLayout(string id, uint flags);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    const uint WM_INPUTLANGCHANGEREQUEST = 0x0050, KLF_ACTIVATE = 0x1;
    public static IntPtr Focused() {
        var fg = GetForegroundWindow();
        var info = new GUITHREADINFO();
        info.cbSize = Marshal.SizeOf(typeof(GUITHREADINFO));
        if (GetGUIThreadInfo(GetWindowThreadProcessId(fg, IntPtr.Zero), ref info) && info.focus != IntPtr.Zero) return info.focus;
        return fg;
    }
    // The language of the layout the focused window types with (0x040D is Hebrew).
    public static int Language() { return (int)(GetKeyboardLayout(GetWindowThreadProcessId(Focused(), IntPtr.Zero)).ToInt64() & 0xFFFF); }
    public static IntPtr Load(string id) { return LoadKeyboardLayout(id, KLF_ACTIVATE); }
    public static void Request(IntPtr layout) {
        PostMessage(Focused(), WM_INPUTLANGCHANGEREQUEST, IntPtr.Zero, layout);
        PostMessage(GetForegroundWindow(), WM_INPUTLANGCHANGEREQUEST, IntPtr.Zero, layout);
    }
}
public static class Wins {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
    // Every top-level window of the process, visible or not: "handle visible class title".
    public static string[] Of(uint process) {
        var found = new System.Collections.Generic.List<string>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid == process) {
                var t = new System.Text.StringBuilder(256); GetWindowText(h, t, 256);
                var c = new System.Text.StringBuilder(256); GetClassName(h, c, 256);
                found.Add(h.ToInt64() + " visible=" + IsWindowVisible(h) + " class=" + c + " title=" + t);
            }
            return true;
        }, IntPtr.Zero);
        return found.ToArray();
    }
}
"@

$A = [System.Windows.Automation.AutomationElement]
$Tree = [System.Windows.Automation.TreeScope]::Descendants

function Get-Window {
    for ($i = 0; $i -lt 120; $i++) {
        $p = Get-Process -Id $ProcessId -ErrorAction Stop
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) { return $A::FromHandle($p.MainWindowHandle) }
        Start-Sleep -Milliseconds 500
    }
    # What there is instead, so the log says why: the process's own windows (a hidden main
    # window, a dialog), its child processes (the WebView2 ones) and the crash log.
    Write-Host "--- windows of process $ProcessId ---"
    [Wins]::Of([uint32]$ProcessId) | ForEach-Object { Write-Host $_ }
    Write-Host "--- its child processes ---"
    Get-CimInstance Win32_Process -Filter "ParentProcessId = $ProcessId" |
        ForEach-Object { Write-Host "$($_.ProcessId) $($_.Name)" }
    $crashes = Join-Path $env:LOCALAPPDATA "il.diagnosticvault.desktop\crash-log.txt"
    if (Test-Path $crashes) { Write-Host "--- crash log ---"; Get-Content $crashes | Write-Host }
    throw "the app shows no window"
}

function Show-Names($root) {
    $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
    $names = foreach ($e in $all) { if ($e.Current.Name) { "$($e.Current.ControlType.ProgrammaticName): $($e.Current.Name)" } }
    Write-Host "--- what the window shows ---"
    $names | Select-Object -First 200 | ForEach-Object { Write-Host $_ }
}

# The first text field whose name matches. A label shows the same name ("סיסמה" above the
# field) and cannot take the keyboard, so only an edit control that can is taken.
function Find-Field($root, [string] $pattern, [string] $what, [int] $seconds = 60) {
    $edit = [System.Windows.Automation.ControlType]::Edit
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            $c = $e.Current
            if ($c.ControlType -eq $edit -and $c.IsKeyboardFocusable -and $c.IsEnabled -and $c.Name -match $pattern) { return $e }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "not found: $what ($pattern)"
}

# The first element whose name matches and that can be pressed. Text inside a button (or a
# heading with the same words) shows the same name and cannot, so only one that can is taken.
function Invoke-Named($root, [string] $pattern, [string] $what, [int] $seconds = 60) {
    $invoke = [System.Windows.Automation.InvokePattern]::Pattern
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            if ($e.Current.Name -notmatch $pattern) { continue }
            $p = $null
            if ($e.TryGetCurrentPattern($invoke, [ref] $p)) {
                $p.Invoke()
                Write-Host "pressed: $($e.Current.Name)"
                return
            }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "nothing to press: $what ($pattern)"
}

# A menu and its items: a menu button opens on a key or a pointer press, not on the click
# Invoke sends, and offers no Invoke at all; an item invoked while its menu settles may be
# gone before it acts. So with -Keyboard (and for anything that cannot be invoked) the
# element takes the focus and Enter, as she would choose it from the keyboard.
function Press-Named($root, [string] $pattern, [string] $what, [int] $seconds = 60, [switch] $Keyboard) {
    $invoke = [System.Windows.Automation.InvokePattern]::Pattern
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $all = $root.FindAll($Tree, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            # A menu that is still opening replaces its items: one found a moment ago may be
            # gone by the time it is pressed. Then it is looked for again.
            try {
                $c = $e.Current
                if ($c.Name -notmatch $pattern -or -not $c.IsEnabled) { continue }
                $p = $null
                $keys = $c.IsKeyboardFocusable -and ($Keyboard -or -not $e.TryGetCurrentPattern($invoke, [ref] $p))
                if (-not $keys -and $e.TryGetCurrentPattern($invoke, [ref] $p)) {
                    $p.Invoke()
                    Write-Host "pressed: $($c.Name)"
                    return
                }
                if ($keys) {
                    Focus-Field $e $what
                    [Keys]::Enter()
                    Write-Host "pressed with Enter: $($c.Name)"
                    return
                }
            }
            catch {
                $x = $_.Exception
                while ($x -and $x -isnot [System.Windows.Automation.ElementNotAvailableException]) { $x = $x.InnerException }
                if (-not $x) { throw }
                Write-Host "(gone before it was pressed, looking again: $what)"
                break
            }
        }
        Start-Sleep -Milliseconds 300
    }
    Show-Names $root
    throw "nothing to press: $what ($pattern)"
}

# An item of the menu that is open, chosen from the keyboard alone: a menu opened with Enter
# puts the focus on its first item, and the arrow moves it down one item at a time (it stops
# at the last one). The items are not looked up and focused from outside: the menu replaces
# them while it opens, and a focus sent to one that is gone lands outside the menu and closes it.
function Choose-InMenu([string] $pattern, [string] $what, [int] $seconds = 20) {
    $deadline = (Get-Date).AddSeconds($seconds)
    Start-Sleep -Milliseconds 800
    while ((Get-Date) -lt $deadline) {
        $f = [System.Windows.Automation.AutomationElement]::FocusedElement
        $name = ""
        try { if ($null -ne $f) { $name = $f.Current.Name } } catch { }
        if ($name -match $pattern) {
            [Keys]::Enter()
            Write-Host "chosen with Enter: $name"
            return
        }
        [Keys]::Down()
        Start-Sleep -Milliseconds 500
    }
    Show-Names $window
    throw "not reached from the keyboard: $what ($pattern); the focus is in $(Focused-Name)"
}

# Where the keys go now, for the log when they went elsewhere.
function Focused-Name {
    $f = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($null -eq $f) { return "(nothing)" }
    return "$($f.Current.ControlType.ProgrammaticName) '$($f.Current.Name)'"
}

# Focus the field, and keep at it until it holds the focus: a dialog that opens moves the
# focus to its first field by itself, a moment after it shows.
function Focus-Field($element, [string] $what) {
    Start-Sleep -Milliseconds 1000
    for ($i = 0; $i -lt 20; $i++) {
        $element.SetFocus()
        Start-Sleep -Milliseconds 250
        if ($element.Current.HasKeyboardFocus) {
            Start-Sleep -Milliseconds 250
            if ($element.Current.HasKeyboardFocus) { return }
        }
    }
    throw "the focus does not stay in $what; it is in $(Focused-Name)"
}

function Type-Into($element, [string] $text, [int] $pauseMs) {
    Focus-Field $element "the field"
    foreach ($c in $text.ToCharArray()) {
        [Keys]::Char($c)
        if ($pauseMs -gt 0) { Start-Sleep -Milliseconds $pauseMs }
    }
}

# The text in a field: its value, or for a long text box that offers only the text pattern,
# the whole of its text.
function Value-Of($element) {
    $p = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref] $p)) {
        return $p.Current.Value
    }
    if ($element.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref] $p)) {
        return $p.DocumentRange.GetText(-1)
    }
    throw "the text field offers neither its value nor its text"
}

$window = Get-Window
[Keys]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null

# 1. Open the vault.
# Not `$password`: PowerShell names ignore case, and that is the [string] parameter above.
$passwordField = Find-Field $window '^סיסמה$' "the password field" 90
Type-Into $passwordField $Password 0
[Keys]::Enter()

# 2. The case, then its materials, then "הדבקת טקסט" for one of them (since round 49 it is in
#    the menu of each kind of material, here the earlier assessments).
Invoke-Named $window 'בתיק של' "a case on the cases screen"
Invoke-Named $window 'חומרים$' "the materials stage"
Press-Named $window '^הוספה לאבחונים קודמים' "the add menu of the earlier assessments" -Keyboard
Choose-InMenu '^הדבקת טקסט' "paste text for a material"
$field = Find-Field $window '^הטקסט$' "the text field of the material"

# 3. 500 Hebrew characters, one key at a time, words and punctuation as she writes them.
$words = @("הילד", "שיחק", "בחצר", "עם", "חברים,", "והתמיד", "במשימה", "גם", "כשהיה", "לו", "קשה.")
$sb = New-Object System.Text.StringBuilder
for ($i = 0; $sb.Length -lt $Characters; $i++) {
    if ($sb.Length -gt 0) { [void]$sb.Append(" ") }
    [void]$sb.Append($words[$i % $words.Count])
}
$text = $sb.ToString().Substring(0, $Characters)
$started = Get-Date
Type-Into $field $text 15
$typed = ((Get-Date) - $started).TotalSeconds

# 4. Exactly what was typed, once the window caught up.
$got = ""
for ($i = 0; $i -lt 20; $i++) {
    $got = Value-Of $field
    if ($got -eq $text) { break }
    Start-Sleep -Milliseconds 250
}
if ($got -ne $text) {
    $n = [Math]::Min($got.Length, $text.Length)
    $at = 0
    while ($at -lt $n -and $got[$at] -eq $text[$at]) { $at++ }
    Write-Host "the focus is in: $(Focused-Name)"
    throw "typing lost or changed keys: $($got.Length) of $($text.Length) characters, first difference at $at"
}
Write-Host ("typing: ok ({0} Hebrew characters in {1:N1} s, none lost)" -f $text.Length, $typed)

# 5. The Hebrew layout, key by key (D-117): the window is switched to Hebrew (KBDHEB, which
#    every Windows has), and each letter is the key it sits on: ש is the A key, and so on.
#    Nothing in the text says how to type it: only the layout turns the keys into Hebrew.
$hebrew = [Layout]::Load("0000040D")
if ($hebrew -eq [IntPtr]::Zero) { throw "the Hebrew keyboard layout could not be loaded" }
$before = [Layout]::Language()
# The field kept the focus from step 3 (the caret at the end of the text): it is not focused
# again, which could move the caret.
if (-not $field.Current.HasKeyboardFocus) { throw "the field lost the focus after typing; it is in $(Focused-Name)" }
for ($i = 0; $i -lt 20 -and [Layout]::Language() -ne 0x040D; $i++) {
    [Layout]::Request($hebrew)
    Start-Sleep -Milliseconds 250
}
if ([Layout]::Language() -ne 0x040D) {
    throw ("the window did not switch to the Hebrew layout (it types with 0x{0:X4}, was 0x{1:X4})" -f [Layout]::Language(), $before)
}
Write-Host ("layout: Hebrew (0x040D) in the focused window, was 0x{0:X4}" -f $before)
# The Israeli standard layout (SI 1452): each letter and the virtual key of its place.
$vk = @{
    [char]'ק' = 0x45; [char]'ר' = 0x52; [char]'א' = 0x54; [char]'ט' = 0x59; [char]'ו' = 0x55; [char]'ן' = 0x49
    [char]'ם' = 0x4F; [char]'פ' = 0x50; [char]'ש' = 0x41; [char]'ד' = 0x53; [char]'ג' = 0x44; [char]'כ' = 0x46
    [char]'ע' = 0x47; [char]'י' = 0x48; [char]'ח' = 0x4A; [char]'ל' = 0x4B; [char]'ך' = 0x4C; [char]'ף' = 0xBA
    [char]'ז' = 0x5A; [char]'ס' = 0x58; [char]'ב' = 0x43; [char]'ה' = 0x56; [char]'נ' = 0x42; [char]'מ' = 0x4E
    [char]'צ' = 0x4D; [char]'ת' = 0xBC; [char]'ץ' = 0xBE; [char]' ' = 0x20
}
$phrase = " הילדה בנתה מגדל גבוה מקוביות וצחקה כשהוא נפל ואז בנתה אותו שוב עם חברה"
$started = Get-Date
foreach ($c in $phrase.ToCharArray()) {
    [Keys]::Key([uint16]$vk[$c])
    Start-Sleep -Milliseconds 15
}
$typed = ((Get-Date) - $started).TotalSeconds
$want = $text + $phrase
$got = ""
for ($i = 0; $i -lt 20; $i++) {
    $got = Value-Of $field
    if ($got -eq $want) { break }
    Start-Sleep -Milliseconds 250
}
if ($got -ne $want) {
    $tail = if ($got.Length -gt $text.Length) { $got.Substring($text.Length) } else { "" }
    Write-Host "the focus is in: $(Focused-Name)"
    Write-Host "typed on the Hebrew layout, the field got: '$tail'"
    throw "typing on the Hebrew layout lost or changed keys: $($got.Length) of $($want.Length) characters"
}
Write-Host ("typing on the Hebrew layout: ok ({0} keys in {1:N1} s, none lost)" -f $phrase.Length, $typed)
