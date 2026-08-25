*** Settings ***
Documentation       manage a mqtt server

Library             Process
Library             MQTTLibrary


*** Variables ***
${container_name}=      ngsi-ld-test-suite-mosquitto-container


*** Keywords ***
Start Mqtt Server
    [Tags]    actor_notification-receiver
    [Arguments]    ${conf_file_name}    ${port}

    ${request} =    Set Variable
    ...    docker run -d --network compose-files_default -e CONF=${conf_file_name} --name "${container_name}" scorpio-test-mosquitto:latest

    Run Process    docker rm -f ${container_name}    shell=yes
    ${result} =    Run Process    ${request}    shell=yes
    IF    ${result.rc} > 0
        Log To Console    The launch of mosquitto encounter an error
        Log To Console    Check that docker is installed
        Log To Console    that the port ${port} is available
        Log To Console    and that no other container named ${container_name} is running
        Log To Console    ${result.stdout}
        Log To Console    ${result.stderr}
    END
    # docker run -d returns before mosquitto listens; without this wait the
    # first MQTT connect races the daemon start and fails with ECONNREFUSED.
    # Poll the broker port until it accepts.
    Wait Until Keyword Succeeds    15s    0.5s    Mqtt Port Is Open    ${port}

Mqtt Port Is Open
    [Arguments]    ${port}
    ${probe} =    Run Process
    ...    python3 -c "import socket; socket.create_connection(('${container_name}', ${port}), 1).close()"
    ...    shell=yes
    Should Be Equal As Integers    ${probe.rc}    0

Stop Mqtt Server
    [Tags]    actor_notification-receiver
    ${request} =    Set Variable    docker container rm -f "${container_name}"
    Run Process    ${request}    shell=yes
