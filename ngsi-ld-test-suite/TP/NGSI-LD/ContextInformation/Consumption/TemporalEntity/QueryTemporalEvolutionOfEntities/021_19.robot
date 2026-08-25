*** Settings ***
Documentation       Check that one can query the temporal evolution of an entity with format or options query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query The Temporal Evolution of An Entity With Format Or Options Query Params


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld


*** Test Cases ***    FORMAT_VALUE    OPTIONS_VALUE    AGGRMETHODS    EXPECTATION_FILENAME
021_19_01 QueryWithFormatOnly
    [Documentation]    Check that one can query the temporal evolution with format query param only
    [Tags]    te-retrieve    5_7_3    6_3_12    since_v1.8.1
    temporalValues    ${EMPTY}    ${EMPTY}    vehicles-temporal-representation-021-19-01.json
021_19_02 QueryWithFormatAndOptions
    [Documentation]    Check that if both format and options are present, the value of the format parameter takes precedence
    [Tags]    te-retrieve    5_7_3    6_3_12    since_v1.8.1
    temporalValues    aggregatedValues    sum    vehicles-temporal-representation-021-19-01.json
021_19_03 QueryWithOptionsOnly
    [Documentation]    Check that one can query the temporal evolution with options query param only
    [Tags]    te-retrieve    5_7_3    6_3_12    since_v1.8.1
    ${EMPTY}    temporalValues    ${EMPTY}    vehicles-temporal-representation-021-19-01.json


*** Keywords ***
Query The Temporal Evolution of An Entity With Format Or Options Query Params
    [Documentation]    Query the temporal evolution of an entity giving format or options query params different values
    [Arguments]    ${format_value}    ${options_value}    ${aggr_methods}    ${expectation_file}
    @{types}=    Create List    Vehicle

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${types}
    ...    options=${options_value}
    ...    format=${format_value}
    ...    aggrMethods=${aggr_methods}
    ...    context=${ngsild_test_suite_context}
    ...    timerel=after
    ...    timeAt=2020-01-01T12:03:00Z

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expectation_file}
    ...    ${response.json()}

Setup Initial Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-19-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Suite Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-19-B
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Suite Variable    ${second_temporal_entity_representation_id}

Delete Initial Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
