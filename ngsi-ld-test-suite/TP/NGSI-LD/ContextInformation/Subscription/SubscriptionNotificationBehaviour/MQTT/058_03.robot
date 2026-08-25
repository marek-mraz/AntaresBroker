*** Settings ***
Documentation       Check that mqtt notification is received with different mqtt version

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
058_03_01 With Default Protocol
    [Tags]    sub-mqtt-notification    5_8_6
    version=${None}
058_03_02 With MQTT 3.1.1 Protocol
    [Tags]    sub-mqtt-notification    5_8_6
    version=mqtt3.1.1
058_03_03 With MQTT 5.0 Protocol
    [Tags]    sub-mqtt-notification    5_8_6
    version=mqtt5.0


*** Keywords ***
Receive Mqtt Notification
    [Documentation]    Check that the broker support different mqtt version
    [Arguments]    ${version}

    Setup Mqtt Subscription    ${endpoint}    ${version}
    Subscribe    topic=${topic}    qos=1

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
    [Arguments]    ${endpoint_uri}    ${version}=${None}
    ${subscription_id}=    Generate Random Subscription Id
    ${entity_id}=    Generate Random Building Entity Id

    ${subscription_payload}=    Load Subscription Sample With Reachable Endpoint
    ...    ${subscription_payload_file_path}
    ...    ${subscription_id}
    ...    ${endpoint_uri}
    ${subscription_payload}=    Set Entity Id In Subscription    ${subscription_payload}    ${entity_id}
    ${notifierInfo}=    Create Dictionary
    IF    $version != $None
        Set To Dictionary    ${notifierInfo}    key    MQTT-Version
        Set To Dictionary    ${notifierInfo}    value    ${version}
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
