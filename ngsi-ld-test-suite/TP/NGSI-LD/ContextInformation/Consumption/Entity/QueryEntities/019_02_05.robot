*** Settings ***
Documentation       Check that one can query entitites via POST Interaction asking for a GeoJSON representation

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entities
Suite Teardown      Delete Entities


*** Variables ***
${vehicle_filename}=                vehicle-simple-attributes.jsonld
${parking_filename}=                parking-simple-attributes.jsonld
${expectation_filename_alt2}=       vehicle-parking-019-02-05.alternative2.geojson
${expectation_filename_alt1}=       vehicle-parking-019-02-05.alternative.geojson
${expectation_filename}=            vehicle-parking-019-02-05.geojson
${vehicle_entity_type}=             https://ngsi-ld-test-suite/context#Vehicle
${parking_entity_type}=             https://ngsi-ld-test-suite/context#OffStreetParking


*** Test Cases ***
019_02_05 Query Several Entities Via POST Interaction Asking For A GeoJSON Representation
    [Documentation]    Check that one can query entities via POST Interaction asking for a GeoJSON representation
    [Tags]    e-query    5_7_2
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    ${vehicle_entity_type}    ${parking_entity_type}
    ${entity_selector}=    Create Dictionary    type=${entity_types_to_be_retrieved}
    @{entities}=    Create List    ${entity_selector}
    ${response}=    Query Entities Via POST
    ...    entities=${entities}
    ...    accept=${CONTENT_TYPE_GEOJSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${alternatives}=    Create List
    ...    ${expectation_filename}
    ...    ${expectation_filename_alt1}
    ...    ${expectation_filename_alt2}
    Check Body With Alternatives
    ...    response_body=${response.json()}
    ...    alternatives=${alternatives}


*** Keywords ***
Create Entities
    ${vehicle_entity_id}=    Catenate    ${vehicle_id_prefix}019-02-05
    Set Suite Variable    ${vehicle_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${vehicle_filename}
    ...    ${vehicle_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${parking_entity_id}=    Catenate    ${parking_id_prefix}019-02-05
    Set Suite Variable    ${parking_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${parking_filename}
    ...    ${parking_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Entities
    Delete Entity    ${vehicle_entity_id}
    Delete Entity    ${parking_entity_id}
