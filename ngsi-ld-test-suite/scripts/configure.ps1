$choco_version = &{choco --version} 2>&1
if ($choco_version -is [System.Management.Automation.ErrorRecord])
{
    powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "iex ((New-Object System.Net.WebClient).DownloadString('https://chocolatey.org/install.ps1'))"

    $env:Path += ";$env:PROGRAMDATA\Chocolatey\bin"
} else {
  echo "Chocolatey already installed"
}

$git_version = &{git --version} 2>&1
if ($git_version -is [System.Management.Automation.ErrorRecord])
{
   choco install git -y
   $env:Path += ";C:\Program Files\Git\cmd"
} else {
   echo "git already installed"
}

# - Install python if it doesn't exists - 
# Define the version of Python to install
$pythonVersion = "3.11.4"

$python_installed_version = &{python --version} 2>&1
if ($python_installed_version -is [System.Management.Automation.ErrorRecord])
{

    # Define the download URL for the Python installer
    $pythonInstallerUrl = "https://www.python.org/ftp/python/$pythonVersion/python-$pythonVersion-amd64.exe"

    # Define the path to download the Python installer
    $installerPath = "$env:TEMP\python-$pythonVersion.exe"

    # Download the Python installer
    Invoke-WebRequest -Uri $pythonInstallerUrl -OutFile $installerPath

    # Install Python silently with the necessary settings to add it to the PATH
    echo "... installing python. This takes some time."
    Start-Process -FilePath $installerPath -ArgumentList "/quiet", "InstallAllUsers=1", "PrependPath=1" -Wait

    # Remove the installer
    Remove-Item $installerPath
    #$env:Path += "C:\Python311\Scripts\;C:\Python311\"
    $env:Path = "C:\Program Files\Python311\Scripts;C:\Program Files\Python311;$env:Path"
} else {
   echo "python is already installed"
}


$python_installed_version = &{python --version} 2>&1
if ($python_installed_version -is [System.Management.Automation.ErrorRecord]) {
    sleep 60
    $python_installed_version = &{python --version} 2>&1
    if ($python_installed_version -is [System.Management.Automation.ErrorRecord]) {
	echo "Restart powershell and run this script again. Python not detected"
	exit 1
    }
}

# Install virtualenv - 
python -m pip install virtualenv

$cloned = &{Test-path .\ngsi-ld-test-suite}

echo "======================="
echo $cloned
echo "======================="

if ( $cloned -eq $false ) {
    echo "------------- doing git clone"
    # Clone the repo - somehow.
    git clone https://github.com/marek-mraz/ngsi-ld-test-suite.git >$null 2>&1
    cd ngsi-ld-test-suite                                           >$null 2>&1
} else {
    cd ngsi-ld-test-suite
    git pull
    Remove-Item -force -recurse .\.venv
}

# Create the virtualenv
virtualenv -ppython3 .venv                               >$null 2>&1

.\.venv\Scripts\activate.bat
pip install -r requirements.txt

