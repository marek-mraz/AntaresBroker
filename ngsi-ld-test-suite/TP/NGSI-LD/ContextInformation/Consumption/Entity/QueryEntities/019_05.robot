*** Settings ***
Documentation       Check that the queried entities by id can be returned in GeoJSON format

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=                        building-location-attribute.jsonld
${expectation_filename}=            two-buildings-location-attribute-normalized-019-05.geojson
${expectation_filename_alt1}=       two-buildings-location-attribute-normalized-019-05.alternative.geojson
${expectation_filename_alt2}=       two-buildings-location-attribute-normalized-019-05.alternative2.geojson
${entity_type}=                     https://ngsi-ld-test-suite/context#Building


*** Test Cases ***
019_05_01 Get An Entity By Id That Can Be Returned In GeoJSON Format
    [Documentation]    Check that the queried entities by id can be returned in GeoJSON format
    [Tags]    e-query    6_3_7
    ${entities_ids_to_be_retrieved}=    Catenate    SEPARATOR=,    ${first_entity_id}    ${second_entity_id}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    ${entity_type}

    ${response}=    Query Entities
    ...    entity_ids=${entities_ids_to_be_retrieved}
    ...    entity_types=${entity_types_to_be_retrieved}
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
Create Initial Entities
    ${first_entity_id}=    Catenate    ${building_id_prefix}019-05-01
    Set Suite Variable    ${first_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    ${second_entity_id}=    Catenate    ${building_id_prefix}019-05-02
    Set Suite Variable    ${second_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
