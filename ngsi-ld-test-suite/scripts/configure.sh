#!/bin/bash
#
#  Command Line Interface to configure the ETSI NGSI-LD Test Suite (https://forge.etsi.org/rep/cim/ngsi-ld-test-suite)
#  For this repository, the commands are merely a convenience script to install the needed services (Python3.11,
#  Virtualenv, Pip, and Git. Additionally, the script clones the repository of the tests and create the proper Python
#  Virtual Environment for the python project based on the requirements.txt file.
#
#  The script will check the OS (MacOS or Ub Ubuntu Linux) and based on it execute the steps to configure the ETSI Test
#  Suite. You can start up with the following command:
#
#  ./configure.sh
#
#  The final steps is the configuration of the ./ngsi-ld-test-suite/resources/variables.py
#  (https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/blob/windows11/resources/variables.py) following the README.md
#  content (https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/tree/windows11?ref_type=heads), activate the Virtual
#  Environment and execute the Robot Tests.


# Function to check if a command is available
command_exists() {
  command -v "$1" >/dev/null 2>&1
}


# Install Homebrew (macOS)
install_homebrew() {
  echo "  - Installing Homebrew..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

  if command_exists brew; then
    echo "    - Brew was installed successfully."
  else
    echo "    - Failed to install Brew."
    exit 1
  fi
}


# Function to prompt for yes/no confirmation
confirm() {
  local prompt="${1:-Are you sure? [Y/n]} "
  local default_value="${2:-Y}"

  read -r -p "$prompt" response
  response=${response:-$default_value}
  response=$(echo "$response" | tr '[:upper:]' '[:lower:]')

  if [[ $response == "y" || $response == "yes" ]]; then
    return 1
  elif [[ $response == "n" || $response == "no" ]]; then
    return 0
  else
    echo "Invalid response. Please enter 'Y' or 'N'."
    confirm "$prompt" "$default_value"
  fi
}

# Create the virtualenv .venv, activate, and install python requirements
configure_virtualenv() {
  echo "Configuring Python Virtual Environment"
  echo "  - Cloning the ETSI repository in the current folder"
  git clone https://github.com/marek-mraz/ngsi-ld-test-suite.git              >/dev/null 2>/dev/null

  cd ngsi-ld-test-suite || {
      echo "Failure, unable to change to the ngsi-ld-test-suite directory, maybe the git clone operation failed...";
      exit 1;
  }

  echo "  - Creating Python Virtual Environment"

  if [ -d "./.venv" ]; then
    echo "    - .venv directory already exists"

    # Prompt for confirmation
    confirm "      Do you want to delete and generate it again? [Y/n]? " "Y"

    # Check the return value
    if [ $? -eq 1 ]; then
      echo "      Deleting previous .venv folder..."
      rm -rf -y .venv                                                          >/dev/null 2>/dev/null
      virtualenv -ppython3.11 .venv                                            >/dev/null 2>/dev/null
    else
      echo "      You declined. Aborting..."
    fi
  else
    virtualenv -ppython3.11 .venv                                              >/dev/null 2>/dev/null
  fi

  # Activate the .venv
  echo "  - Activating .venv"
  source .venv/bin/activate                                                    >/dev/null 2>/dev/null

  # Install the requirements
  echo "  - Installing the python requirements from the project"
  pip3 install -r requirements.txt                                             >/dev/null 2>/dev/null
}


# Configuration script for macOS
execute_macos() {
  echo "macOS operating system, installing requirements..."

  # Check if Homebrew is already installed
  if ! command_exists brew; then
    install_homebrew
  else
    echo "  - Homebrew is already installed."
  fi

  # Check if git is already installed
  if ! command_exists git; then
    echo "  - Installing git..."
    brew install git --quiet                                                   >/dev/null 2>/dev/null

    # Check that git was successfully installed
    if command_exists git; then
      echo "    - Git installed successfully"
    else
      echo "      - Failed to install Git."
      exit 1
    fi
  else
    echo "  - Git is already installed."
  fi

  # Check if Python 3.11 is already installed
  if command_exists python3.11; then
    echo "  - Python 3.11 is already installed."
  else
    # Install Python 3.11 for macOS, it includes also the installation of pip3
    echo "  - Installing Python 3.11 (with pip) and Virtualenv for macOS..."
    brew install python@3.11 virtualenv --quiet                                >/dev/null 2>/dev/null

    # Check if Python 3.11 was installed successfully
    if command_exists python3.11; then
      echo "    - Python 3.11 was installed successfully on macOS."
    else
      echo "    - Failed to install Python 3.11 on macOS."
      exit 1
    fi

    # Check if Pip was installed successfully
    if command_exists pip3; then
      echo "    - Pip3 was installed successfully."
    else
      echo "    - Failed to install Pip3."
      exit 1
    fi

    # Check if Virtualenv was installed successfully
    if command_exists virtualenv; then
      echo "    - Virtualenv was installed successfully."
    else
      echo "    - Failed to install Virtualenv."
      exit 1
    fi
  fi

  echo

  # Create the virtualenv .venv, activate, and install python requirements
  configure_virtualenv
}

# Configuration script for Linux
execute_linux() {
  echo "Linux operating system, installing requirements..."

  # Check if git is already installed
  if ! command_exists git; then
    echo "  - Installing Git."
    sudo apt-get install -y -qq git                                            >/dev/null 2>/dev/null

    # Check that git was successfully installed
    if command_exists git; then
      echo "    - Git installed successfully."
    else
      echo "    - Failed to install Git."
      exit 1
    fi
  else
    echo "  - Git is already installed."
  fi

  # Check if Python 3.11 is already installed
  if command_exists python3.11; then
    echo "  - Python 3.11 is already installed."
  else
    # Install Python 3.11 for Ubuntu
    echo "  - Installing Python 3.11, Pip, and Virtualenv for Ubuntu..."
    sudo add-apt-repository -y ppa:deadsnakes/ppa                              >/dev/null 2>/dev/null
    sudo apt-get update     -y -qq                                             >/dev/null 2>/dev/null
    sudo apt-get install    -y -qq python3.11 python3-pip python3-virtualenv   >/dev/null 2>/dev/null

    # Check if Python 3.11 was installed successfully
    if command_exists python3.11; then
      echo "    - Python 3.11 was installed successfully."
    else
      echo "    - Failed to install Python 3.11."
      exit 1
    fi

    # Check if Pip was installed successfully
    if command_exists pip3; then
      echo "    - Pip3 was installed successfully."
    else
      echo "    - Failed to install Pip3."
      exit 1
    fi

    # Check if Virtualenv was installed successfully
    if command_exists virtualenv; then
      echo "    - Virtualenv was installed successfully."
    else
      echo "    - Failed to install Virtualenv."
      exit 1
    fi

  fi

  echo

  # Create the virtualenv .venv, activate, and install python requirements
  configure_virtualenv
}



# Determine the operating system
os=$(uname -s)

# Execute the script based in the OS
if [[ $os == "Darwin" ]]; then
  execute_macos
  echo
  echo "Ready to use..."
elif [[ $os == "Linux" ]]; then
  execute_linux
  echo
  echo "Ready to use..."
else
  echo "Unsupported operating system: $os"
  exit 1
fi

exit 0
