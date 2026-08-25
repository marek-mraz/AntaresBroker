*** Settings ***
Documentation       Check that retrieve entity returns an error when queried with faulty format or options query params

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Retrieve Entity With Faulty Format Or Options Query Params


*** Variables ***
${filename}=    building-simple-attributes.jsonld


*** Test Cases ***    FORMAT_VALUE    OPTIONS_VALUE
018_16_01 RetriveWithInvalidFormat
    [Documentation]    Check that retrieving an entity with invalid format query param returns an error
    [Tags]    e-retrieve    5_7_1    6_3_7    6_3_20    since_v1.8.1
    invalidFormat    ${EMPTY}
018_16_02 RetriveWithInvalidOptions
    [Documentation]    Check that retrieving an entity with invalid options query param returns an error
    [Tags]    e-retrieve    5_7_1    6_3_7    6_3_20    since_v1.8.1
    ${EMPTY}    invalidOptions
018_16_03 RetriveWithAtLeastOneInvalidOptions
    [Documentation]    Check that retrieving an entity with at least one invalid options query param returns an error
    [Tags]    e-retrieve    5_7_1    6_3_7    6_3_20    since_v1.8.1
    ${EMPTY}    keyValues,invalid
018_16_04 RetriveWithOneInvalidQueryParam
    [Documentation]    Check that retrieving an entity with one invalid query param returns an error
    [Tags]    e-retrieve    5_7_1    6_3_7    6_3_20    since_v1.8.1
    keyValues    invalid


*** Keywords ***
Retrieve Entity With Faulty Format Or Options Query Params
    [Documentation]    Retrieve Entity with faulty format or options query params and check the returned error
    [Arguments]    ${format_value}    ${options_value}
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    format=${format_value}
    ...    options=${options_value}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
