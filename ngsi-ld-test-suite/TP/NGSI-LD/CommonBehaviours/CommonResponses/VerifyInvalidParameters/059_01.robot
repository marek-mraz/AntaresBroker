*** Settings ***
Documentation       Check that sending an invalid parameter returns a 400 invalidRequest

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Call endpoint with invalid parameter


*** Variables ***
${filename}=    building-minimal.json


*** Test Cases ***    METHOD    ENDPOINT    FILENAME    ADDITIONAL_QUERY_PARAM
059_01_01 CreateEntity
    [Tags]    e-create    5_6_1    6_3_20    since_v1.7.1
    POST    entities    entities/building-minimal.json
059_01_02 QueryEntities
    [Tags]    e-query    5_7_2    6_3_20    since_v1.7.1
    GET    entities    additionalParam=type=Building
059_01_03 CreateTemporalEntity
    [Tags]    te-create    5_6_11    6_3_20    since_v1.7.1
    POST    ${TEMPORAL_ENTITIES_ENDPOINT_PATH}    entities/building-minimal.json
059_01_04 QueryTemporalEntities
    [Tags]    te-query    5_7_2    6_3_20    since_v1.7.1
    GET    ${TEMPORAL_ENTITIES_ENDPOINT_PATH}    additionalParam=type=Building
059_01_05 RetrieveEntityTypes
    [Tags]    ed-types    5_7_5    6_3_20    since_v1.7.1
    GET    ${ENTITIES_TYPES_ENDPOINT_PATH}
059_01_06 CreateSubscription
    [Tags]    sub-create    5_8_1    6_3_20    since_v1.7.1
    POST    subscriptions    subscriptions/subscription.jsonld
059_01_07 QuerySubscriptions
    [Tags]    sub-create    5_8_1    6_3_20    since_v1.7.1
    GET    subscriptions
059_01_08 PurgeEntities
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1
    DELETE    entities


*** Keywords ***
Call endpoint with invalid parameter
    [Documentation]    Check that sending an invalid parameter returns a 400 invalidRequest
    [Arguments]    ${method}    ${endpoint}    ${filename}=${EMPTY}    ${additionalParam}=${EMPTY}
    ${response}=    Call Api Endpoint With Invalid Parameter
    ...    ${method}
    ...    ${endpoint}
    ...    ${filename}
    ...    ${additionalParam}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
