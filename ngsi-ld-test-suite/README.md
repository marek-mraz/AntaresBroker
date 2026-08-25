# ETSI NGSI-LD Test Suite

This repository contains the ETSI NGSI-LD Test Suite to test the compliance of the Context Brokers with the
specification of the ETSI NGSI-LD API.

## Contents

<details>

<summary><strong>Details</strong></summary>

-   [Install the Test Suite](#install-the-test-suite)
-   [Configure the Test Suite](#configure-the-test-suite)
-   [Execute the Test Suite](#execute-the-ngsi-ld-test-suite)
-   [Use the Test Suite Management (tsm) tool](#test-suite-management-tsm)
-   [Contribute to the Test Suite](#contribute-to-the-test-suite)
-   [Frameworks and libraries used in the project](#frameworks-and-libraries-used-in-the-project)
-   [Useful links](#useful-links)   
-   [LICENSE](#license)

</details>


## Install the Test Suite

To install the ETSI NGSI-LD Test Suite, download the configuration script:

- For macOS and Ubuntu, download the following file:

```$ curl https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/scripts/configure.sh > configure.sh```

- For Windows, using PowerShell download the following file (curl is an alias for Invoke-WebRequest in PowerShell):

```> curl https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/scripts/configure.ps1 > configure.ps1```

- For macOS and Ubuntu, be sure that you have the proper execution permissions of the file and the user is included 
in the sudoers group, then execute the following script:

```$ configure.sh```

- In Windows 11, using PowerShell, make sure you can run the script:

```> powershell -executionpolicy bypass configure.ps1```

These scripts develop a set of operations, the same in the different Operating System, which consist of:
- Installation of the corresponding software requirements:
  - Python3.11
  - Git
  - Virtualenv
  - Pip
- Cloning the NGSI-LD Test Suite repository in the local folder and create the corresponding Python Virtual Environment
taking into account the content of `requirements.txt` file. Further details on each library can be found in 
[PyPi](https://pypi.org/) and 
[Robot Framework Standard Libraries](http://robotframework.org/robotframework/#standard-libraries).


## Configure the Test Suite

In the `resources/variables.py` file, configure the following parameters:

- `url` : It is the url of the context broker which is to be tested (including the `/ngsi-ld/v1` path, 
e.g., http://localhost:8080/ngsi-ld/v1).
- `temporal_api_url` : This is the url of the GET temporal operation API, in case that a Context Broker splits 
this portion of the API (e.g., http://localhost:8080/ngsi-ld/v1).
- `ngsild_test_suite_context` : This is the url of the default context used in the ETSI NGSI-LD requests 
(e.g. 'https://raw.githubusercontent.com/marek-mraz/ngsi-ld-test-suite/main/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld').
- `notification_server_host` and `notification_server_port` : This is the address and port used to create the local 
server to listen to notifications (the address must be accessible by the context broker). 
- `mqtt_broker_host` : This is the address used to contact the MQTT broker where notifications are sent. 
Default value: `127.0.0.1`.
- `mqtt_broker_non_default_port` : This is the non-default port used to check that notifications are correctly sent
when the MQTT broker does not listen on the default port.
Default value: `8085`.
- `context_source_host` and `context_source_port` : The address and port used for the context source. 
Default value: `0.0.0.0` and `8086`.
- `context_server_host` and `context_server_port` : The address and port used for the context server provider. 
Default value: `0.0.0.0` and `8087`.
- `core_context`: The default cached core context used by the Brokers.
- `delete_temporal_on_core_delete`: Whether the Temporal Representation of an Entity should be deleted when an Entity is deleted.
Default value: True

When you locally execute the tests, you can leave the default values as they are. NGSI-LD Test Suite provides
mockup services to provide the notification functionality, and therefore the `notification_server_host` can be
assigned to `0.0.0.0`. In case that you deploy the Context Broker on a docker engine, you need to specify the
URL of the Docker Host. In this case you have two options to specify the IP address of your host machine:

- (Windows, macOS) As of Docker v18.03+ you can use the `host.docker.internal` hostname to connect to your 
Docker host.
- (Linux) Need to extract the IP of the `docker0` interface. Execute the following command:
``` ifconfig docker0 | grep inet | awk '{print $2}' ``` and that IP is the one that you need to use for 
notification purposes.

Optionally, extra variables can be configured to automatically create a GitHub Issue for failed Test Cases in the 
Context Broker's repository, using a custom Robot Listener class. If you don't need this functionality, remove those
variables from the file.

As an explanation of the process, the GitHub Issue will be created if there is no other issue in the repository with
the same name or there is an issue with the same name, but it is not closed.

To create these issues, the [GitHub REST API](https://docs.github.com/en/rest) is used. For this purpose, 
the authentication process is using a personal access token. The necessary variables are the following:
* `github_owner`: Your GitHub user account. 
* `github_broker_repo` : The corresponding URL of the Context Broker repository.
* `github_token` : Your personal access token. Please take a look at the GitHub documentation if you want to generate 
your own [personal access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens).

## Execute the Test Suite

The first operation that you have to develop is the activation of the Python Virtual Environment through the
execution of the following command in macOS or Ubuntu:

```$ source .venv/bin/activate```

In the case of Windows, you need to execute the following command:

```> .\venv\Scripts\activate.bat ```

Now, you can launch the tests with the following command on macOS or Linux:

```$ robot --outputdir ./results .```  

For a Windows system, you can launch the tests with the following command:

```> robot --outputdir .\results .\TP\NGSI-LD```

For more running instructions, please consult [scripts/run_tests.sh](./scripts/run_tests.sh) or [scripts/run_tests.ps1](./scripts/run_tests.ps1)

To have the whole messages, it is necessary to modify the width of the test execution output with the option 
--consolewidth (-W). The default width is 78 characters.

```$ robot --consolewidth 150  .```

The messages in the console are clear and without noise, only the strictly necessary (request and response to the 
Context Brokers and a nice message which shows the difference between two documents when they are). However, it can be 
challenging to follow all the messages in the console when there is a lot of testing and a lot of Context Broker calls. 
This is why a command to redirect the console output to a file can be used. You must add a chevron at the end of the 
test launch command followed by the file name.

```$ robot . > results.log```

> **Note:** if you want to deactivate the Python Virtual Environment, just execute the command in 
> macOS and Ubuntu systems:
> 
> ```console
> $ deactivate
> ```
> And in Windows system:
> 
> ```console
> > .venv\scripts\deactivate.bat
> ```

### Specific test requirements

- Mqtt tests (058) : 
  - Mqtt tests launch a Mosquitto container with docker, thus it requires docker to be installed and running
- Temporal pagination (020_13, 020_14, 021_15 & 021_16) :
  - The pagination tests assume that it will be triggered by 1 or 2 entities with 2 attributes with 20 instances each
    (40 instances)
  - Non-pagination tests assume that it will **not** be triggered by 2 entities with 2 attributes with 5 instances each
    (20 instances)
  - You should configure your broker accordingly
- Interoperability tests (IOP_CNF_*) :
  - The Interoperability tests assume that it will manage multiple brokers
  - Multiple NGSI-LD brokers will be running simultaneously on different ports
  - Terminal will pass the broker URLs as a variable

  The following example shows how to run an interoperability test with multiple brokers: 

  ```
    robot --variable b1_url:http://localhost:8080/ngsi-ld/v1 \
      --variable b2_url:http://localhost:8081/ngsi-ld/v1 \
      --variable b3_url:http://localhost:8082/ngsi-ld/v1 \
      --variable b4_url:http://localhost:8083/ngsi-ld/v1 \
      --variable b5_url:http://localhost:8084/ngsi-ld/v1 \
      ./TP/NGSI-LD/Interoperability/Provision/Entities/CreateEntity/IOP_CNF_01_01.robot
  ```

### Generate output file details only for failed tests

It is possible to generate a report only for the failed tests through the use of a specific listener in the execution
of the robot framework. For example, if you want to execute the test suite number 043 and generate the report, you can
execute the following command:

```robot  --listener libraries/ErrorListener.py --outputdir ./results ./TP/NGSI-LD/CommonBehaviours/043.robot```

It will generate a specific `errors.log` file into the `results` folder with the description of the different steps 
developed and the mismatched observed on them.

## Test Suite Management (tsm)

The tsm (Test Suite Management) tool is a command-line script designed to help selectively manage and execute test
cases from the NGSI-LD Test Suite — particularly useful when a Context Broker only implements a subset of the
NGSI-LD API.

You can read more about it in the [TSM.md](TSM.md) file.

## Contribute to the Test Suite

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for the full contribution guide, covering commit message conventions,
the pre-commit formatting hook, Test Case authoring rules, documentation update steps, and the self-review checklist.

## Frameworks and libraries used in the project

* [Robot Framework](https://github.com/robotframework/robotframework)
* [JSON Library](https://github.com/robotframework-thailand/robotframework-jsonlibrary)
* [Requests Library](https://github.com/MarketSquare/robotframework-requests)
* [Deep Diff](https://github.com/seperman/deepdiff)
* [HttpCtrl Library](https://github.com/annoviko/robotframework-httpctrl)
* [Robotidy Library ](https://github.com/MarketSquare/robotframework-tidy)


## Useful links

* [Robot Framework User Guide](http://robotframework.org/robotframework/latest/RobotFrameworkUserGuide.html#output-file)   


## LICENSE

Copyright 2021-2026 ETSI

The content of this repository and the files contained are released under the [ETSI Software License](LICENSE) a 
(BSD-3-Clause LICENSE).