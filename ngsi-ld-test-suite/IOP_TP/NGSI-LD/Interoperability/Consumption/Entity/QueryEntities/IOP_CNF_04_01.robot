*** Settings ***
Documentation       Five brokers are set up A, B, C, D and E. A has two registrations, one auxiliary for the entity created in B, one inclusive for the entity created in C. B has two registrations, one redirect for the entity created in D and one redirect for the entity created in E. C has two exclusive registrations to E.
...                 Check that the entities found in B, C, D and E can be queried from A via HTTP GET and said entities have the same attributes as the ones queried from A.
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations
Test Teardown       Delete Entities and Delete Registrations

*** Variables ***
${first_parking_location_name_payload_filename}       interoperability/offstreet-parking1-location-and-name.jsonld
${second_parking_location_name_payload_filename}      interoperability/offstreet-parking2-location-and-name.jsonld
${first_full_parking_payload_filename}                interoperability/offstreet-parking1-full.jsonld
${second_full_parking_payload_filename}               interoperability/offstreet-parking2-full.jsonld
${auxiliary_registration_payload_file_path}           csourceRegistrations/interoperability/context-source-registration-auxiliary-2.jsonld
${inclusive_registration_payload_file_path}           csourceRegistrations/interoperability/context-source-registration-inclusive-1.jsonld
${first_exclusive_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-exclusive-2.jsonld
${second_exclusive_registration_payload_file_path}    csourceRegistrations/interoperability/context-source-registration-exclusive-3.jsonld
${first_redirect_registration_payload_file_path}      csourceRegistrations/interoperability/context-source-registration-redirect-1.jsonld
${second_redirect_registration_payload_file_path}     csourceRegistrations/interoperability/context-source-registration-redirect-2.jsonld
${b1_url}
${b2_url}
${b3_url}
${b4_url}
${b5_url}

*** Test Cases ***
IOP_CNF_04_01 Query Entities Of Type OffStreetParking Via GET
    [Documentation]    Pre-conditions: No user context. Data only on leaves. D contains OffStreetParking:1 with location and name only and OffStreetParking:2. E contains OffStreetParking:1 and OffStreetParking:2 with location and name only.
    ...                Registrations established: Auxiliary in A to B and Inclusive in A to C. Redirect in B to D and Redirect in B to E. Exclusive in C to E.
    [Tags]    since_v1.6.1    iop    4_3_3    cf_06    additive-inclusive    additive-auxiliary    proxy-exclusive    proxy-redirect    4_3_6    5_7_2    6_4_3_1

    #Client queries all entities with type OffStreetParking in A and checks for a successful response.
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b1_url}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

    &{payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${first_parking_payload}=    Get From Dictionary    ${payload}    ${entity_id}
    ${second_parking_payload}=    Get From Dictionary    ${payload}    ${second_entity_id}
    Should Contain    ${first_parking_payload}    location
    Should Contain    ${second_parking_payload}    availableSpotsNumber
    Should Contain    ${second_parking_payload}    totalSpotsNumber
    Should Contain    ${second_parking_payload}    location

    #Client queries all entities with type OffStreetParking in D and E.
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b4_url}    context=${ngsild_test_suite_context}
    ${payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${expected_entity1}=    Get From Dictionary    ${payload}    ${second_entity_id}
    ${response}=    Query Entities    entity_types=OffStreetParking    broker_url=${b5_url}    context=${ngsild_test_suite_context}
    ${payload}=    Evaluate    {i['id']: i for i in ${response.json()}}
    ${expected_entity2}=    Get From Dictionary    ${payload}    ${entity_id}
    ${expected_entity3}=    Get From Dictionary    ${payload}    ${second_entity_id}

    #Client checks that the attributes of the entities in A are the same as the ones in D and E.
    Should Be Equal    ${first_parking_payload}[location]    ${expected_entity2}[location]
    Should Be Equal    ${second_parking_payload}[availableSpotsNumber]    ${expected_entity1}[availableSpotsNumber]
    Should Be Equal    ${second_parking_payload}[totalSpotsNumber]    ${expected_entity1}[totalSpotsNumber]
    Should Be Equal    ${second_parking_payload}[location]    ${expected_entity3}[location]

*** Keywords ***
Setup Initial Context Source Registrations
    ${entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${entity_id}
    ${second_entity_id}=    Generate Random Parking Entity Id
    Set Suite Variable    ${second_entity_id}

    ${response}=    Create Entity    ${first_parking_location_name_payload_filename}    ${entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_full_parking_payload_filename}    ${second_entity_id}    broker_url=${b4_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${first_full_parking_payload_filename}    ${entity_id}    broker_url=${b5_url}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${second_parking_location_name_payload_filename}    ${second_entity_id}    broker_url=${b5_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id1}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id1}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id1}
    ...    ${auxiliary_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b2_url}
    ...    mode=auxiliary
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${first_redirect_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b4_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id3}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id3}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id3}
    ...    ${second_redirect_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b5_url}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b2_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id4}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id4}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id4}
    # Type-only registration (no entity_id): per NGSI-LD 4.3.6.1 the forwarded request is
    # narrowed to the registered scope, so an id-scoped reg would starve the other entity.
    ...    ${inclusive_registration_payload_file_path}
    ...    broker_url=${b3_url}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b1_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id5}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id5}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id5}
    ...    ${first_exclusive_registration_payload_file_path}
    ...    entity_id=${second_entity_id}
    ...    broker_url=${b5_url}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id6}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id6}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id6}
    ...    ${second_exclusive_registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    broker_url=${b5_url}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}    broker_url=${b3_url}
    Check Response Status Code    201    ${response.status_code}

    # Registrations propagate asynchronously to the broker's in-VM registry cache — an
    # immediate query/create can race ahead of the last registration (flaky aux/inclusive merges).
    Sleep    1s

Delete Entities And Delete Registrations
    Delete Context Source Registration    ${registration_id1}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id2}    broker_url=${b2_url}
    Delete Context Source Registration    ${registration_id3}    broker_url=${b2_url}
    Delete Context Source Registration    ${registration_id4}    broker_url=${b1_url}
    Delete Context Source Registration    ${registration_id5}    broker_url=${b3_url}
    Delete Context Source Registration    ${registration_id6}    broker_url=${b3_url}
    Delete Entity    ${entity_id}    broker_url=${b4_url}
    Delete Entity    ${second_entity_id}    broker_url=${b4_url}
    Delete Entity    ${entity_id}    broker_url=${b5_url}
    Delete Entity    ${second_entity_id}    broker_url=${b5_url}