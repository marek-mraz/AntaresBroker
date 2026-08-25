*** Settings ***
Documentation       Check that one cannot query the temporal evolution of entities when called with faulty format or options query params

Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Query The Temporal Evolution of An Entity With Faulty Format Or Options Query Params


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    FORMAT_VALUE    OPTIONS_VALUE
021_20_01 QueryWithInvalidFormat
    [Documentation]    Check that one cannot query with an invalid format query param
    [Tags]    te-retrieve    5_7_3    6_3_12    6_3_20    since_v1.8.1
    invalidFormat    ${EMPTY}
021_20_02 QueryWithInvalidOptions
    [Documentation]    Check that one cannot query with an invalid options query param
    [Tags]    te-retrieve    5_7_3    6_3_12    6_3_20    since_v1.8.1
    ${EMPTY}    invalidOptions
021_20_03 QueryWithAtLeastOneInvalidOptions
    [Documentation]    Check that one cannot query with at least one invalid options query param
    [Tags]    te-retrieve    5_7_3    6_3_12    6_3_20    since_v1.8.1
    ${EMPTY}    temporalValues,invalidOptions
021_20_04 QueryWithOneInvalidQueryParam
    [Documentation]    Check that one cannot query with at least one invalid query param
    [Tags]    te-retrieve    5_7_3    6_3_12    6_3_20    since_v1.8.1
    temporalValues    aggregatedValues,invalidOptions


*** Keywords ***
Query The Temporal Evolution of An Entity With Faulty Format Or Options Query Params
    [Documentation]    Query the temporal evolution of an entity with faulty format or options query params and check the returned error
    [Arguments]    ${format_value}    ${options_value}
    @{types}=    Create List    Vehicle
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${temporal_entity_representation_id}

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${types}
    ...    options=${options_value}
    ...    format=${format_value}
    ...    context=${ngsild_test_suite_context}
    ...    timerel=after
    ...    timeAt=2020-01-01T12:03:00Z
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
