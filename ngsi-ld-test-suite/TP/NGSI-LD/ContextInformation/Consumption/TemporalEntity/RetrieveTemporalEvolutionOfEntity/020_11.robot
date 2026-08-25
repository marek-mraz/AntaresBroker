*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity with the aggregated temporal representation

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Initial Temporal Entity
Test Template       Retrieve the temporal evolution of an entity with the aggregated temporal representation


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld


*** Test Cases ***    AGGRMETHODS    AGGRPERIODDURATION    ATTRS    VEHICLE_EXPECTATION_FILE
020_11_01 One Aggregate Method Aggregated By One Hour Duration
    [Tags]    te-retrieve    5_7_3    4_5_19    since_v1.4.1
    avg    PT1H    ${EMPTY}    vehicle-temporal-representation-aggregated-avg-PT1H.json
020_11_02 One Aggregate Method Aggregated By One Hour Duration Asking For One Attribute
    [Tags]    te-retrieve    5_7_3    4_5_19    since_v1.4.1
    avg    PT1H    fuelLevel    vehicle-temporal-representation-aggregated-avg-PT1H-fuelLevel.json
020_11_03 Multiple Aggregate Methods Aggregated By One Hour Duration
    [Tags]    te-retrieve    5_7_3    4_5_19    since_v1.4.1
    avg,max    PT1H    ${EMPTY}    vehicle-temporal-representation-aggregated-avg-max-PT1H.json
020_11_04 Multiple Aggregate Methods Aggregated By One Day Duration
    [Tags]    te-retrieve    5_7_3    4_5_19    since_v1.4.1
    min,max    P1D    ${EMPTY}    vehicle-temporal-representation-aggregated-min-max-P1D.json


*** Keywords ***
Retrieve the temporal evolution of an entity with the aggregated temporal representation
    [Documentation]    Check that one can retrieve the temporal evolution of an entity with the aggregated temporal representation
    [Arguments]    ${aggr_methods}    ${aggr_period_duration}    ${attrs}    ${vehicle_expectation_file}
    @{options}=    Create List    aggregatedValues
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    attrs=${attrs}
    ...    options=${options}
    ...    aggrMethods=${aggr_methods}
    ...    aggrPeriodDuration=${aggr_period_duration}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    ${vehicle_expectation_file}
    ...    ${temporal_entity_representation_id}
    ...    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
