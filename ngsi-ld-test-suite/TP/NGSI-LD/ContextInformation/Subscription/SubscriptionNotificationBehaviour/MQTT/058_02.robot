*** Settings ***
Documentation       Check that the mqtt notification is received with different qos

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/mqttUtils/MqttUtils.resource

Test Setup          Start Mqtt Server And Connect
Test Teardown       After Test
Test Template       Receive MQTT Notification


*** Variables ***
${subscription_payload_file_path}       subscriptions/subscription-building-entities-default.jsonld
${entity_building_filepath}             building-simple-attributes.jsonld
${fragment_filename}                    airQualityLevel-fragment.jsonld
${topic}                                ngsild-test-suite/topic
${endpoint}                             mqtt://${mqtt_broker_host}/${topic}
${config_filename}                      mosquitto.conf


*** Test Cases ***
058_02_01 With Default QoS
    [Tags]    sub-mqtt-notification    5_8_6
    qos=${None}
058_02_02 With QoS 1
    [Tags]    sub-mqtt-notification    5_8_6
    qos=1
058_02_03 With QoS 2
    [Tags]    sub-mqtt-notification    5_8_6
    qos=2


*** Keywords ***
Receive MQTT Notification
    [Documentation]    Check that the broker supports different quality of service (qos)
    [Arguments]    ${qos}

    Setup Mqtt Subscription    ${endpoint}    qos=${qos}
    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}

    ${listenMessages}=    Listen    ${topic}
    Check Messages Contain One Instance    ${listenMessages}

Start Mqtt Server And Connect
    Start Mqtt Server    ${config_filename}    1883
    Connect    ${mqtt_broker_host}    1883
    Subscribe    topic=${topic}    qos=1

After Test
    Delete Initial Subscriptions
    Delete Initial Entity
    Stop Mqtt Server
    Disconnect

Setup Mqtt Subscription
    [Arguments]    ${endpoint_uri}    ${qos}=${None}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id

    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${endpoint_uri}
    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}
    IF    ${qos} != ${None}
        ${notifierInfo}=    Create Dictionary
        Set To Dictionary    ${notifierInfo}    key    MQTT-QoS
        Set To Dictionary    ${notifierInfo}    value    ${qos}
        ${notifierInfo}=    Create List    ${notifierInfo}
        ${subscription_payload}=    Add Object To Json
        ...    ${subscription_payload}
        ...    $.notification.endpoint.notifierInfo
        ...    ${notifierInfo}
    END

    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_building_filepath}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    Sleep    1s
    ${response}=    Create Subscription From Subscription Payload    ${subscription_payload}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

    Sleep    1s

Delete Initial Subscriptions
    Delete Subscription    ${subscription_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
