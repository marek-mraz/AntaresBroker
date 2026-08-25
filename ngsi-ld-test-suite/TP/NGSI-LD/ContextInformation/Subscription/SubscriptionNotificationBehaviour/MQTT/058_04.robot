*** Settings ***
Documentation       Check that the mqtt notification payload is well formed

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/mqttUtils/MqttUtils.resource

Test Setup          Start Mqtt Server And Connect
Test Teardown       After Test
Test Template       Receive Mqtt Notification


*** Variables ***
${subscription_payload_file_path}       subscriptions/subscription-building-entities-default.jsonld
${entity_building_filepath}             building-simple-attributes.jsonld
${fragment_filename}                    airQualityLevel-fragment.jsonld
${topic}                                ngsild-test-suite/topic
${endpoint}                             mqtt://${mqtt_broker_host}/${topic}
${config_filename}                      mosquitto.conf


*** Test Cases ***
058_04_01 Basic Subscription
    [Tags]    sub-mqtt-notification    5_8_6
    ${EMPTY}


*** Keywords ***
Receive Mqtt Notification
    [Documentation]    Check that the broker send well formed data
    [Arguments]    ${arg}

    ${response}=    Update Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    ${listenMessages}=    Listen    ${topic}
    ${firstmessage}=    Set Variable    ${listenMessages}[0]
    ${message}=    Convert String To Json    ${firstmessage}

    Check Message Contain Key    ${message}    metadata
    Check Message Contain Key    ${message}    body
    Check Message Contain Key    ${message}[metadata]    Content-Type
    Check Message Contain Key    ${message}[metadata]    Link
    Check Message Field Equal    ${message}[metadata][test-receiver-key]    test-receiver-info
    Check Message Field Equal    ${message}[body][type]    Notification
    Check Message Contain Key    ${message}[body]    data
    Check Message Contain Key    ${message}[body][data][0]    id
    Check Message Contain Key    ${message}[body][data][0]    type

Start Mqtt Server And Connect
    Start Mqtt Server    ${config_filename}    1883
    Setup Mqtt Subscription    ${endpoint}

    Connect    ${mqtt_broker_host}    1883
    Subscribe    topic=${topic}    qos=1

After Test
    Delete Initial Subscriptions
    Delete Initial Entity
    Stop Mqtt Server
    Disconnect

Setup Mqtt Subscription
    [Arguments]    ${endpoint_uri}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id

    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${endpoint_uri}
    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}

    Set Suite Variable    ${subscription_id}

    Set Suite Variable    ${entity_id}
    ${receiverInfo}=    Create Dictionary
    Set To Dictionary    ${receiverInfo}    key    test-receiver-key
    Set To Dictionary    ${receiverInfo}    value    test-receiver-info
    ${receiverInfo}=    Create List    ${receiverInfo}
    ${subscription_payload}=    Add Object To Json
    ...    ${subscription_payload}
    ...    $.notification.endpoint.receiverInfo
    ...    ${receiverInfo}

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
