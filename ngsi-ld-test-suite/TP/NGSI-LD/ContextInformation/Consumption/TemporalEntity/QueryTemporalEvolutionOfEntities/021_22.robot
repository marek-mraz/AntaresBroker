*** Settings ***
Documentation       Check that a BadRequestDataException is returned if the Query Temporal Entities request is invalid with respect to pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Query Temporal Entities With Invalid Pick Or Omit Query Params Usage


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld
${bus_payload_file}=                2020-08-bus-temporal-representation.jsonld


*** Test Cases ***    PICK    OMIT    ATTRS
021_22_01 RetrieveWithSameEntityMemberInPickAndOmit
    [Documentation]    Check that a BadRequestDataException is returned if an entity member is present in pick and omit
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    speed    speed    ${EMPTY}
021_22_02 RetrieveWithPickAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if pick and attrs query params are present
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    speed    ${EMPTY}    fuelLevel
021_22_03 RetrieveWithOmitAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if omit and attrs query params are present
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    ${EMPTY}    speed    fuelLevel


*** Keywords ***
Query Temporal Entities With Invalid Pick Or Omit Query Params Usage
    [Documentation]    Check that a BadRequestDataException is returned if the Query Temporal Entities request is invalid with respect to pick and omit query params
    [Arguments]    ${pick}    ${omit}    ${attrs}

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=Building,Vehicle
    ...    timerel=after
    ...    timeAt=1970-01-01T00:00:00Z
    ...    pick=${pick}
    ...    omit=${omit}
    ...    attrs=${attrs}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Setup Initial Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-22-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-22-B
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_temporal_entity_representation_id}
    ${third_temporal_entity_representation_id}=    Catenate    ${BUS_ID_PREFIX}021-22-C
    ${create_response3}=    Create Temporal Representation Of Entity
    ...    ${bus_payload_file}
    ...    ${third_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Test Variable    ${third_temporal_entity_representation_id}

Delete Initial Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${third_temporal_entity_representation_id}
